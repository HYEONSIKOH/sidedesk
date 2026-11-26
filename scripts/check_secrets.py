"""저장소와 배포 파일에서 비밀값과 개인 정보를 찾는다. 하나라도 나오면 끝 코드 1.

사용: python3 -I scripts/check_secrets.py [--repo] [--artifact 파일]... [--keychain]
- --repo: git 에 올라간 파일 전부
- --artifact: .app 폴더, .dmg(맥), .zip, .apk, 그 밖의 파일
- --keychain: 맥 열쇠고리의 기상청 키와 같은 값이 있는지(로컬 전용, 값은 출력하지 않음)
금지어 목록은 저장소 밖 ~/.config/sidedesk/deny-words(줄마다 하나)에서만 읽는다.
찾은 값은 가려서 보인다.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import tempfile
import zipfile
from dataclasses import dataclass, field
from pathlib import Path

CHUNK = 1 << 20
OVERLAP = 256
SHOW = 20

# (종류, 정규식). 바이트 정규식이라 바이너리 안에서도 찾는다.
RULES: list[tuple[str, bytes]] = [
    ("개인 키", rb"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
    ("GitHub 토큰", rb"gh[pousr]_[A-Za-z0-9]{36}"),
    ("GitHub 토큰", rb"github_pat_[A-Za-z0-9_]{60,}"),
    ("GitLab 토큰", rb"glpat-[A-Za-z0-9_-]{20}"),
    ("AWS 키", rb"AKIA[0-9A-Z]{16}"),
    ("Slack 토큰", rb"xox[abprs]-[A-Za-z0-9-]{10,}"),
    ("AI 키", rb"sk-ant-[A-Za-z0-9_-]{20,}"),
    ("AI 키", rb"\bsk-[A-Za-z0-9]{32,}"),
    ("기상청 키", rb"(?:serviceKey|authKey)=[A-Za-z0-9%+/=_-]{20,}"),
    ("기상청 키", rb"[A-Za-z0-9+/]{80,}=="),
    # 시험 표본의 중립 이름(me)과 CI 러너 이름은 허용
    ("홈 경로", rb"/Users/(?!Shared/|runner/|me/|<)[A-Za-z0-9._-]+"),
    ("홈 경로", rb"/home/(?!runner/|me/|<)[A-Za-z0-9._-]+"),
    ("홈 경로", rb"[A-Za-z]:\\{1,2}Users\\{1,2}(?!Public\\|runneradmin\\|me\\|<)[A-Za-z0-9._-]+"),
    # 이름표 뒤 따옴표 안 값만(코드의 변수 이름은 걸리지 않게)
    ("일련번호", rb"(?i)(?:serial(?:no|_number|number)?|\xec\x9d\xbc\xeb\xa0\xa8\xeb\xb2\x88\xed\x98\xb8)[\"']?\s*[:=]\s*[\"'][A-Za-z0-9]{8,}[\"']"),
    ("메일", rb"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}"),
]

MAIL_ALLOW = re.compile(
    rb"(?i)^(noreply@anthropic\.com|.*@example\.(com|org|net)|.*@users\.noreply\.github\.com|info@open-meteo\.com)$"
)

# 넓게 열지 않는다. (저장소 안 경로, 종류) 짝만.
ALLOW: set[tuple[str, str]] = set()


@dataclass
class Found:
    where: str
    kind: str
    shown: str


@dataclass
class Scan:
    words: list[bytes] = field(default_factory=list)
    secret: bytes | None = None
    found: list[Found] = field(default_factory=list)

    def rules(self) -> list[tuple[str, re.Pattern[bytes]]]:
        rules = [(kind, re.compile(pattern)) for kind, pattern in RULES]
        for word in self.words:
            escaped = re.escape(word)
            # 짧은 낱말은 낱말 경계로만
            pattern = rb"(?i)\b" + escaped + rb"\b" if len(word) < 3 else rb"(?i)" + escaped
            rules.append(("금지어", re.compile(pattern)))
        if self.secret:
            rules.append(("실제 기상청 키", re.compile(re.escape(self.secret))))
        return rules


def mask(kind: str, value: bytes) -> str:
    if kind in ("금지어", "실제 기상청 키"):
        return "(가림)"
    text = value.decode("utf-8", "replace")
    return text[:4] + "…"


def variants(data: bytes) -> list[bytes]:
    """UTF-8 그대로와, UTF-16LE 로 보이는 글자를 좁힌 것"""
    narrowed = re.sub(rb"([\x20-\x7e])\x00", rb"\1", data)
    return [data] if narrowed == data else [data, narrowed]


def scan_bytes(scan: Scan, where: str, data: bytes, rules) -> None:
    seen: set[tuple[str, bytes]] = set()
    for text in variants(data):
        for kind, rule in rules:
            for match in rule.finditer(text):
                value = match.group(0)
                if kind == "메일" and (
                    MAIL_ALLOW.match(value) or value.split(b"@")[0].count(b".") > 2
                ):
                    # 앞부분에 점이 셋 넘으면 자바, 코틀린 패키지 이름(androidx.….EXTRA@…)
                    continue
                if (where.split(":")[0], kind) in ALLOW:
                    continue
                if (kind, value) in seen:
                    continue
                seen.add((kind, value))
                line = text.count(b"\n", 0, match.start()) + 1
                place = f"{where}:{line}" if b"\0" not in text[:4096] else f"{where}@{match.start()}"
                scan.found.append(Found(place, kind, mask(kind, value)))


def scan_file(scan: Scan, where: str, path: Path, rules) -> None:
    with open(path, "rb") as handle:
        tail = b""
        while True:
            chunk = handle.read(CHUNK)
            if not chunk:
                break
            scan_bytes(scan, where, tail + chunk, rules)
            tail = chunk[-OVERLAP:]


def scan_zip(scan: Scan, where: str, path: Path, rules) -> None:
    with zipfile.ZipFile(path) as archive:
        for info in archive.infolist():
            if info.is_dir():
                continue
            inner = f"{where}!{info.filename}"
            data = archive.read(info)
            if info.filename.lower().endswith((".zip", ".apk", ".jar", ".aar")):
                with tempfile.NamedTemporaryFile(suffix=".zip") as nested:
                    nested.write(data)
                    nested.flush()
                    scan_zip(scan, inner, Path(nested.name), rules)
            else:
                scan_bytes(scan, inner, data, rules)


def scan_tree(scan: Scan, where: str, root: Path, rules) -> None:
    for path in sorted(root.rglob("*")):
        if path.is_file() and not path.is_symlink():
            scan_artifact(scan, f"{where}/{path.relative_to(root)}", path, rules)


def scan_dmg(scan: Scan, where: str, path: Path, rules) -> None:
    mount = Path(tempfile.mkdtemp(prefix="sidedesk-dmg-"))
    attached = False
    try:
        subprocess.run(
            ["hdiutil", "attach", "-readonly", "-nobrowse", "-mountpoint", str(mount), str(path)],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        attached = True
        scan_tree(scan, where, mount, rules)
    finally:
        if attached:
            subprocess.run(["hdiutil", "detach", str(mount), "-force"], stdout=subprocess.DEVNULL)
        try:
            mount.rmdir()
        except OSError:
            pass


def scan_artifact(scan: Scan, where: str, path: Path, rules) -> None:
    name = path.name.lower()
    if path.is_dir():
        scan_tree(scan, where, path, rules)
    elif name.endswith(".dmg"):
        scan_dmg(scan, where, path, rules)
    elif name.endswith((".zip", ".apk", ".aab", ".jar")) and zipfile.is_zipfile(path):
        scan_zip(scan, where, path, rules)
    else:
        scan_file(scan, where, path, rules)


def repo_files(root: Path) -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "-z"], cwd=root, check=True, capture_output=True
    ).stdout
    return [name for name in out.decode("utf-8").split("\0") if name]


def deny_words() -> list[bytes]:
    path = Path.home() / ".config" / "sidedesk" / "deny-words"
    if not path.is_file():
        return []
    words = []
    for line in path.read_text("utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            words.append(line.encode("utf-8"))
    return words


def keychain_secret() -> bytes | None:
    if sys.platform != "darwin":
        return None
    out = subprocess.run(
        ["security", "find-generic-password", "-s", "io.github.hyeonsikoh.sidedesk", "-a", "kma", "-w"],
        capture_output=True,
    )
    value = out.stdout.strip()
    return value or None


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="비밀값과 개인 정보 찾기")
    parser.add_argument("--repo", action="store_true")
    parser.add_argument("--artifact", action="append", default=[])
    parser.add_argument("--keychain", action="store_true")
    args = parser.parse_args(argv)
    root = Path(__file__).resolve().parent.parent
    scan = Scan(words=deny_words())
    notes = []
    if not scan.words:
        notes.append("금지어 목록 없음, 건너뜀")
    if args.keychain:
        scan.secret = keychain_secret()
        if scan.secret is None:
            notes.append("열쇠고리에 기상청 키 없음, 건너뜀")
    rules = scan.rules()
    if args.repo:
        for name in repo_files(root):
            path = root / name
            if path.is_file():
                scan_file(scan, name, path, rules)
    for artifact in args.artifact:
        path = Path(artifact)
        scan_artifact(scan, path.name, path, rules)
    for note in notes:
        print(note)
    by_kind: dict[str, int] = {}
    for found in scan.found:
        by_kind[found.kind] = by_kind.get(found.kind, 0) + 1
        if by_kind[found.kind] <= SHOW:
            print(f"{found.where} {found.kind} {found.shown}")
    if scan.found:
        print("찾음: " + ", ".join(f"{kind} {count}" for kind, count in by_kind.items()))
        return 1
    print("비밀값과 개인 정보 없음")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
