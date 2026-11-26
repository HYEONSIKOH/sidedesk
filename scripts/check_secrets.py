"""저장소와 배포 파일에서 비밀값과 개인 정보를 찾는다. 하나라도 나오면 끝 코드 1.

사용: python3 -I scripts/check_secrets.py [--repo] [--history] [--artifact 파일]... [--forbid 글자]...
- --repo: git 에 올라간 파일 전부(지금 트리)
- --history: git 이력의 모든 파일 내용(로컬에서 공개 전에 한 번)
- --artifact: .app 폴더, .dmg(맥), .zip, .apk, 그 밖의 파일. 산출물은 홈 경로 허용 없이 엄격하게 본다
- --forbid: 이 글자가 산출물에 있으면 실패(빌드한 곳의 실제 경로 등)
금지어 목록은 저장소 밖 ~/.config/sidedesk/deny-words(줄마다 하나)에서만 읽는다.
비밀 저장소(열쇠고리 등)에서 값을 꺼내지 않는다. 키는 모양 규칙으로만 찾는다.
찾은 값은 가려서 보인다. 그림 속 글자는 볼 수 없어 사람이 본다.
파이썬 3.9 이상에서 돈다(맥 기본 /usr/bin/python3).
"""

from __future__ import annotations

import argparse
import os
import re
import struct
import subprocess
import sys
import tempfile
import zipfile
import zlib
from dataclasses import dataclass, field
from pathlib import Path

CHUNK = 1 << 20
OVERLAP = 256
SHOW = 20
# 압축 안 압축은 두 단계까지 연다.
DEPTH = 2

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
    # 이름표 뒤 따옴표 안 값만(코드의 변수 이름은 걸리지 않게)
    ("일련번호", rb"(?i)(?:serial(?:no|_number|number)?|\xec\x9d\xbc\xeb\xa0\xa8\xeb\xb2\x88\xed\x98\xb8)[\"']?\s*[:=]\s*[\"'][A-Za-z0-9]{8,}[\"']"),
    # 기계어 바이트가 우연히 맞지 않게: 앞은 글자로 시작해 세 글자 이상, 끝은 2~6글자 영문
    ("메일", rb"(?<![A-Za-z0-9._%+-])[A-Za-z][A-Za-z0-9._%+-]{2,63}@[A-Za-z0-9-]{2,63}(?:\.[A-Za-z0-9-]{2,63})*\.[A-Za-z]{2,6}(?![A-Za-z0-9])"),
]

# 저장소: 시험 표본의 중립 이름(me)과 CI 러너 이름은 허용
REPO_HOMES = [
    rb"/Users/(?!Shared/|runner/|me/|<)[A-Za-z0-9._-]+",
    rb"/home/(?!runner/|me/|<)[A-Za-z0-9._-]+",
    rb"(?i)[A-Z]:[\\/]{1,2}Users[\\/]{1,2}(?!Public[\\/]|runneradmin[\\/]|me[\\/]|<)[A-Za-z0-9._-]+",
]
# 산출물: 경로 바꾸기가 빠지면 CI 러너 경로도 잡히게 허용 없이
ARTIFACT_HOMES = [
    rb"/Users/(?!Shared/)[A-Za-z0-9._-]+",
    rb"/home/[A-Za-z0-9._-]+",
    rb"(?i)[A-Z]:[\\/]{1,2}Users[\\/]{1,2}(?!Public[\\/])[A-Za-z0-9._-]+",
]
# 산출물만: 맨몸 64자 16진(공공데이터포털 새 형식 키 모양). 저장소의 lock 파일 해시와 겹쳐 저장소엔 쓰지 않는다.
ARTIFACT_ONLY = [("기상청 키(16진)", rb"(?<![0-9a-fA-F])[0-9a-f]{64}(?![0-9a-fA-F])")]

MAIL_ALLOW = re.compile(
    # info@open-meteo.com: 의문점에 적은 Open-Meteo 의 공개 연락처(출처 표기)
    rb"(?i)^(noreply@anthropic\.com|info@open-meteo\.com|.*@example\.(com|org|net)|.*@users\.noreply\.github\.com)$"
)

# 넓게 열지 않는다. (저장소 안 경로, 종류) 짝만.
ALLOW: set[tuple[str, str]] = set()
# 메일 끝이 이것이면 패키지 이름으로 보지 않는다.
MAIL_ENDS = {b"com", b"net", b"org", b"io", b"kr", b"co", b"dev", b"app", b"me", b"ai", b"jp", b"cn", b"edu", b"gov"}

ELF_COMPRESSED = 0x800


@dataclass
class Found:
    where: str
    kind: str
    shown: str


@dataclass
class Scan:
    words: list[bytes] = field(default_factory=list)
    # 산출물이면 엄격하게
    strict: bool = False
    forbid: list[bytes] = field(default_factory=list)
    found: list[Found] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)

    def rules(self, raw: bool = False) -> list[tuple[str, re.Pattern[bytes]]]:
        """`raw` 는 압축된 바이트(APK, dmg 파일 자체)용. 거의 무작위라 짧은 영문 금지어가 우연히 맞는다."""
        rules = [(kind, re.compile(pattern)) for kind, pattern in RULES]
        homes = ARTIFACT_HOMES if self.strict else REPO_HOMES
        rules += [("홈 경로", re.compile(pattern)) for pattern in homes]
        if self.strict:
            rules += [(kind, re.compile(pattern)) for kind, pattern in ARTIFACT_ONLY]
        for word in self.words:
            ascii_word = word.isascii()
            # 압축 바이트에서 영문 3글자 이하는 무작위로도 자주 맞아(30MB 에 열 번 남짓) 뺀다.
            if raw and ascii_word and len(word) < 4:
                continue
            escaped = re.escape(word)
            # 짧은 영문 낱말과 압축 바이트에서는 앞뒤 경계로만. 한글은 그대로 맞춘다.
            if ascii_word and (raw or len(word) < 3):
                pattern = rb"(?i)(?<![A-Za-z0-9])" + escaped + rb"(?![A-Za-z0-9])"
            else:
                pattern = rb"(?i)" + escaped
            rules.append(("금지어", re.compile(pattern)))
            # UTF-16 꼴(바이너리 XML, 윈도우 자원, 문자열 풀)로도 찾는다. 영문은 좁히기가
            # 넷 이상 이어질 때만이라 짧은 낱말을 놓치므로 영문도 따로 넣는다.
            if not raw:
                wide = re.escape(word.decode("utf-8").encode("utf-16-le"))
                rules.append(("금지어", re.compile(rb"(?i)" + wide if ascii_word else wide)))
        for value in self.forbid:
            rules.append(("빌드한 곳 경로", re.compile(re.escape(value))))
        return rules


# 앞머리가 정해진 모양이라 앞 몇 글자를 보여도 값이 드러나지 않는 종류와 그 길이
PREFIX = {
    "개인 키": 10,
    "GitHub 토큰": 4,
    "GitLab 토큰": 6,
    "AWS 키": 4,
    "Slack 토큰": 5,
    "AI 키": 3,
    "홈 경로": 6,
}


def mask(kind: str, value: bytes) -> str:
    """메일, 키 값, 일련번호, 금지어 같은 것은 한 글자도 보이지 않는다."""
    if kind not in PREFIX:
        return "(가림)"
    return value.decode("utf-8", "replace")[: PREFIX[kind]] + "…"


def variants(data: bytes) -> list[bytes]:
    """그대로와, UTF-16LE 로 보이는 글자를 좁힌 것. 글자 뒤 0 을 지우므로 짝수, 홀수 위치 모두 덮는다."""
    narrowed = re.sub(
        rb"(?:[\x20-\x7e]\x00){4,}", lambda m: m.group(0)[::2], data
    )
    return [data] if narrowed == data else [data, narrowed]


def package_name(value: bytes) -> bool:
    """앞에 점이 셋 넘고 끝이 메일 끝이 아니면 자바, 코틀린 이름(androidx.….EXTRA@androidx.activity)"""
    local, _, domain = value.partition(b"@")
    return local.count(b".") > 2 and domain.rsplit(b".", 1)[-1].lower() not in MAIL_ENDS


def allowed(where: str, kind: str) -> bool:
    if where.startswith("이력 "):
        where = where.split(" ", 2)[2]
    return (where.split("!")[0].split(":")[0], kind) in ALLOW


NEWLINE = b"\n"


def scan_bytes(scan: Scan, where: str, data: bytes, rules, seen=None, line_base: int = 0) -> None:
    """`seen` 을 넘기면 한 파일의 여러 조각에서 같은 값을 한 번만 센다."""
    if seen is None:
        seen = set()
    binary = b"\0" in data
    for text in variants(data):
        for kind, rule in rules:
            for match in rule.finditer(text):
                value = match.group(0)
                if kind == "메일" and (MAIL_ALLOW.match(value) or package_name(value)):
                    continue
                if allowed(where, kind) or (kind, value) in seen:
                    continue
                seen.add((kind, value))
                if binary:
                    place = f"{where}@{match.start()}"
                else:
                    line = line_base + text.count(NEWLINE, 0, match.start()) + 1
                    place = f"{where}:{line}"
                scan.found.append(Found(place, kind, mask(kind, value)))


def elf_compressed(data: bytes) -> bool:
    """64비트 리틀 엔디언 ELF 에 압축 구역(SHF_COMPRESSED)이 있는지"""
    if data[:4] != b"\x7fELF" or data[4] != 2 or data[5] != 1:
        return False
    shoff = struct.unpack_from("<Q", data, 0x28)[0]
    size, count = struct.unpack_from("<HH", data, 0x3A)
    for index in range(count):
        at = shoff + index * size
        if at + 16 > len(data):
            break
        if struct.unpack_from("<Q", data, at + 8)[0] & ELF_COMPRESSED:
            return True
    return b".zdebug" in data


def png_texts(data: bytes) -> list[bytes]:
    """PNG 의 압축된 글 조각(zTXt, iTXt)을 풀어 돌려준다."""
    texts = []
    at = 8
    while at + 8 <= len(data):
        length, kind = struct.unpack_from(">I4s", data, at)
        body = data[at + 8 : at + 8 + length]
        at += 12 + length
        try:
            if kind == b"zTXt":
                texts.append(zlib.decompress(body.split(b"\0", 1)[1][1:]))
            elif kind == b"iTXt":
                keyword, rest = body.split(b"\0", 1)
                if rest[0] == 1:
                    lang_and_more = rest[2:].split(b"\0", 2)
                    texts.append(zlib.decompress(lang_and_more[2]))
        except (IndexError, zlib.error):
            texts.append(b"")
    return texts


def scan_data(scan: Scan, where: str, data: bytes, rules) -> None:
    """메모리에 다 올린 한 파일"""
    scan_bytes(scan, where, data, rules)
    if data[:8] == b"\x89PNG\r\n\x1a\n":
        for text in png_texts(data):
            scan_bytes(scan, f"{where}#글조각", text, rules)
    if elf_compressed(data):
        scan.found.append(Found(where, "압축 구역(안을 못 봄)", "-"))


def scan_file(scan: Scan, where: str, path: Path, rules) -> None:
    with open(path, "rb") as handle:
        head = handle.read(8)
        handle.seek(0)
        # 그림과 ELF 는 통째로 보고(구역 머리, 글 조각), 나머지는 조각으로
        if head[:8] == b"\x89PNG\r\n\x1a\n" or head[:4] == b"\x7fELF":
            scan_data(scan, where, handle.read(), rules)
            return
        tail = b""
        seen: set = set()
        lines = 0
        while True:
            chunk = handle.read(CHUNK)
            if not chunk:
                break
            scan_bytes(scan, where, tail + chunk, rules, seen, lines - tail.count(NEWLINE))
            lines += chunk.count(NEWLINE)
            # 겹침을 짝수로 두어 UTF-16 정렬을 지킨다.
            tail = chunk[-OVERLAP:]


def scan_zip(scan: Scan, where: str, path: Path, rules, depth: int = 0) -> None:
    # 파일 자체도 훑는다(APK 서명 블록의 인증서, 항목 이름, 주석은 항목 내용이 아니다).
    scan_file(scan, f"{where}(묶음)", path, scan.rules(raw=True))
    with zipfile.ZipFile(path) as archive:
        names = "\n".join(info.filename for info in archive.infolist()).encode("utf-8")
        scan_bytes(scan, f"{where}(이름)", names, rules)
        for info in archive.infolist():
            if info.is_dir():
                continue
            inner = f"{where}!{info.filename}"
            data = archive.read(info)
            nested = info.filename.lower().endswith((".zip", ".apk", ".jar", ".aar"))
            if nested and depth < DEPTH:
                with tempfile.NamedTemporaryFile(suffix=".zip", delete=False) as handle:
                    handle.write(data)
                try:
                    scan_zip(scan, inner, Path(handle.name), rules, depth + 1)
                finally:
                    os.unlink(handle.name)
            elif nested:
                scan.found.append(Found(inner, "너무 깊은 압축(안을 못 봄)", "-"))
            else:
                scan_data(scan, inner, data, rules)


def scan_tree(scan: Scan, where: str, root: Path, rules) -> None:
    # 바로 가기는 따라가지 않는다(dmg 의 /Applications 등).
    names = "\n".join(
        str(Path(folder, name).relative_to(root))
        for folder, dirs, files in os.walk(root, followlinks=False)
        for name in dirs + files
    ).encode("utf-8")
    scan_bytes(scan, f"{where}(이름)", names + NEWLINE + root.name.encode("utf-8"), rules)
    for folder, dirs, files in os.walk(root, followlinks=False):
        for name in sorted(dirs + files):
            path = Path(folder) / name
            if path.is_symlink():
                scan.notes.append(f"바로 가기 건너뜀: {where}/{path.relative_to(root)}")
        for name in sorted(files):
            path = Path(folder) / name
            if not path.is_symlink():
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
    if path.is_symlink():
        scan.notes.append(f"바로 가기 건너뜀: {where}")
    elif path.is_dir():
        scan_tree(scan, where, path, rules)
    elif name.endswith(".dmg"):
        # 파일 자체(볼륨 이름 등 압축 밖 머리)도 훑는다.
        scan_file(scan, f"{where}(묶음)", path, scan.rules(raw=True))
        scan_dmg(scan, where, path, rules)
    elif name.endswith((".zip", ".apk", ".aab", ".jar", ".aar")) and zipfile.is_zipfile(path):
        scan_zip(scan, where, path, rules)
    else:
        scan_file(scan, where, path, rules)


def git(root: Path, *args: str, data: bytes | None = None) -> bytes:
    return subprocess.run(
        ["git", *args], cwd=root, check=True, capture_output=True, input=data
    ).stdout


def repo_files(root: Path) -> list[str]:
    out = git(root, "ls-files", "-z")
    return [name for name in out.decode("utf-8").split("\0") if name]


def scan_history(scan: Scan, root: Path, rules) -> None:
    """이력의 모든 파일 내용(같은 내용은 한 번)"""
    names: dict[str, str] = {}
    for line in git(root, "rev-list", "--all", "--objects").decode("utf-8", "replace").splitlines():
        sha, _, name = line.partition(" ")
        if name and sha not in names:
            names[sha] = name
    blobs = [
        sha
        for sha, kind in (
            line.split(" ")[:2]
            for line in git(
                root, "cat-file", "--batch-check=%(objectname) %(objecttype)",
                data="\n".join(names).encode(),
            ).decode().splitlines()
        )
        if kind == "blob"
    ]
    for sha in blobs:
        data = git(root, "cat-file", "blob", sha)
        scan_data(scan, f"이력 {sha[:8]} {names[sha]}", data, rules)


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


def warn(text: str) -> None:
    # GitHub Actions 에서는 작업 요약에 경고로 보인다.
    print(f"::warning::{text}" if os.environ.get("GITHUB_ACTIONS") == "true" else text)


def report(scan: Scan) -> int:
    for note in scan.notes:
        print(note)
    by_kind: dict[str, int] = {}
    for found in scan.found:
        by_kind[found.kind] = by_kind.get(found.kind, 0) + 1
        if by_kind[found.kind] <= SHOW:
            print(f"{found.where} {found.kind} {found.shown}")
    if scan.found:
        print("찾음: " + ", ".join(f"{kind} {count}" for kind, count in by_kind.items()))
        return 1
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="비밀값과 개인 정보 찾기")
    parser.add_argument("--repo", action="store_true")
    parser.add_argument("--history", action="store_true")
    parser.add_argument("--artifact", action="append", default=[])
    parser.add_argument("--forbid", action="append", default=[])
    args = parser.parse_args(argv)
    if not (args.repo or args.history or args.artifact):
        parser.error("--repo, --history, --artifact 중 하나는 있어야 합니다")
    root = Path(__file__).resolve().parent.parent
    words = deny_words()
    if not words:
        warn("금지어 목록 없음, 금지어 검사 건너뜀")
    failed = 0
    if args.repo or args.history:
        scan = Scan(words=words)
        rules = scan.rules()
        if args.repo:
            for name in repo_files(root):
                path = root / name
                if path.is_file() and not path.is_symlink():
                    scan_file(scan, name, path, rules)
        if args.history:
            scan_history(scan, root, rules)
        failed |= report(scan)
    if args.artifact:
        forbid = [value.encode("utf-8") for value in args.forbid if value]
        scan = Scan(words=words, strict=True, forbid=forbid)
        rules = scan.rules()
        for artifact in args.artifact:
            path = Path(artifact)
            scan_artifact(scan, path.name, path, rules)
        failed |= report(scan)
    if not failed:
        print("비밀값과 개인 정보 없음")
    return failed


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
