"""check_secrets.py 시험. 표본은 저장소 훑기에 걸리지 않게 조각을 이어 만든다.

사용: python3 -I scripts/test_check_secrets.py
"""

import os
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import check_secrets as cs  # noqa: E402


def j(*parts: str) -> bytes:
    return "".join(parts).encode("utf-8")


def kinds(data: bytes, scan: cs.Scan | None = None) -> list[str]:
    scan = scan or cs.Scan()
    cs.scan_bytes(scan, "x", data, scan.rules())
    return [found.kind for found in scan.found]


class Rules(unittest.TestCase):
    def test_each_kind_is_found(self):
        samples = {
            "개인 키": j("-----BEGIN ", "OPENSSH PRIVATE", " KEY-----"),
            "GitHub 토큰": j("gh", "p_", "a" * 36),
            "GitLab 토큰": j("gl", "pat-", "b" * 20),
            "AWS 키": j("AK", "IA", "ABCDEFGHIJKLMNOP"),
            "Slack 토큰": j("xo", "xb-", "1234567890ab"),
            "AI 키": j("sk", "-ant-", "c" * 24),
            "기상청 키": j("service", "Key=", "d" * 30),
            "홈 경로": j("/Us", "ers/", "alice/work"),
            "일련번호": j('"seri', 'al": "', "R5CT12345678", '"'),
            "메일": j("alice", "@", "mail.test.io"),
        }
        for kind, sample in samples.items():
            with self.subTest(kind=kind):
                self.assertIn(kind, kinds("앞 ".encode() + sample + " 뒤".encode()))

    def test_windows_home_and_utf16(self):
        path = j("C:", "\\", "Users", "\\", "bob", "\\", "app")
        self.assertEqual(kinds(path), ["홈 경로"])
        self.assertEqual(kinds(path.decode().encode("utf-16-le")), ["홈 경로"])

    def test_neutral_and_code_are_not_found(self):
        for clean in [
            j("/Us", "ers/", "me/work/app"),
            j("/Us", "ers/", "runner/work"),
            j("/home/", "runner/x"),
            b"let serial = properties.get(\"SerialNumber\")",
            j("noreply", "@", "anthropic.com"),
            j("a", "@", "example.com"),
            j("text3androidx.activity.result.contract.extra.PERMISSIONS", "@", "androidx.activity.result"),
            b"sk-short",
        ]:
            with self.subTest(clean=clean):
                self.assertEqual(kinds(clean), [])

    def test_values_are_masked(self):
        scan = cs.Scan()
        token = j("gh", "p_", "z" * 36)
        cs.scan_bytes(scan, "x", token, scan.rules())
        self.assertEqual(scan.found[0].shown, token[:4].decode() + "…")
        scan = cs.Scan(words=[j("se", "cretname")])
        cs.scan_bytes(scan, "x", j("a ", "SECRETNAME", " b"), scan.rules())
        self.assertEqual([(f.kind, f.shown) for f in scan.found], [("금지어", "(가림)")])

    def test_short_deny_words_match_whole_words_only(self):
        scan = cs.Scan(words=[b"ab"])
        self.assertEqual(kinds(b"tab cab", scan), [])
        scan = cs.Scan(words=[b"ab"])
        self.assertEqual(kinds(b"x ab y", scan), ["금지어"])

    def test_the_real_key_is_found_and_hidden(self):
        scan = cs.Scan(secret=j("Real", "Key123"))
        cs.scan_bytes(scan, "x", j("..", "RealKey123", ".."), scan.rules())
        self.assertEqual([(f.kind, f.shown) for f in scan.found], [("실제 기상청 키", "(가림)")])


class Artifacts(unittest.TestCase):
    def test_inside_a_zip_and_a_nested_apk(self):
        with tempfile.TemporaryDirectory() as folder:
            inner = Path(folder) / "inner.apk"
            with zipfile.ZipFile(inner, "w") as apk:
                apk.writestr("lib/a.so", b"\x00\x01" + j("/Us", "ers/", "carol/.cargo") + b"\x00")
            outer = Path(folder) / "out.zip"
            with zipfile.ZipFile(outer, "w") as archive:
                archive.write(inner, "app/inner.apk")
                archive.writestr("readme.txt", "깨끗함")
            scan = cs.Scan()
            cs.scan_artifact(scan, "out.zip", outer, scan.rules())
            self.assertEqual(len(scan.found), 1)
            self.assertTrue(scan.found[0].where.startswith("out.zip!app/inner.apk!lib/a.so"))

    def test_a_value_across_chunks_is_found(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "big.bin"
            token = j("gh", "p_", "q" * 36)
            data = b"\x00" * (cs.CHUNK - 10) + token + b"\x00" * 100
            path.write_bytes(data)
            scan = cs.Scan()
            cs.scan_artifact(scan, "big.bin", path, scan.rules())
            self.assertEqual([f.kind for f in scan.found], ["GitHub 토큰"])


class Strict(unittest.TestCase):
    def test_artifacts_allow_no_runner_home_and_catch_forbidden(self):
        runner = j("/Us", "ers/", "runner/work/app")
        self.assertEqual(kinds(runner), [])
        self.assertEqual(kinds(runner, cs.Scan(strict=True)), ["홈 경로"])
        scan = cs.Scan(strict=True, forbid=[j("D:", "\\a\\side")])
        self.assertEqual(kinds(j("x D:", "\\a\\side\\y"), scan), ["빌드한 곳 경로"])

    def test_bare_hex_key_only_in_artifacts(self):
        key = ("0123456789abcdef" * 4).encode()
        self.assertEqual(kinds(key), [])
        self.assertEqual(kinds(key, cs.Scan(strict=True)), ["기상청 키(16진)"])

    def test_korean_deny_word_matches_inside_text(self):
        word = "회사".encode()
        self.assertEqual(kinds("우리회사이름".encode(), cs.Scan(words=[word])), ["금지어"])

    def test_png_text_chunks_are_unpacked(self):
        import struct as st
        import zlib as zl

        def chunk(kind, body):
            return st.pack(">I", len(body)) + kind + body + b"\0\0\0\0"

        hidden = j("/Us", "ers/", "dave/shot")
        png = b"\x89PNG\r\n\x1a\n" + chunk(b"zTXt", b"Comment\0\0" + zl.compress(hidden))
        scan = cs.Scan()
        cs.scan_data(scan, "a.png", png, scan.rules())
        self.assertEqual([(f.where, f.kind) for f in scan.found], [("a.png#글조각:1", "홈 경로")])

    def test_compressed_elf_sections_fail(self):
        scan = cs.Scan()
        cs.scan_data(scan, "a.so", b"\x7fELF\x02\x01" + b"\0" * 64 + b".zdebug_info", scan.rules())
        self.assertEqual([f.kind for f in scan.found], ["압축 구역(안을 못 봄)"])


class Tree(unittest.TestCase):
    def test_links_are_not_followed(self):
        with tempfile.TemporaryDirectory() as folder, tempfile.TemporaryDirectory() as outside:
            Path(outside, "secret.txt").write_bytes(j("gh", "p_", "k" * 36))
            root = Path(folder) / "stage"
            root.mkdir()
            Path(root, "ok.txt").write_text("깨끗함")
            os.symlink(outside, root / "Applications")
            scan = cs.Scan(strict=True)
            cs.scan_artifact(scan, "stage", root, scan.rules())
            self.assertEqual(scan.found, [])
            self.assertTrue(any("Applications" in note for note in scan.notes))

    def test_nesting_deeper_than_the_limit_fails(self):
        with tempfile.TemporaryDirectory() as folder:
            inner = Path(folder) / "0.zip"
            with zipfile.ZipFile(inner, "w") as z:
                z.writestr("a.txt", "x")
            for level in range(1, cs.DEPTH + 2):
                outer = Path(folder) / f"{level}.zip"
                with zipfile.ZipFile(outer, "w") as z:
                    z.write(inner, "in.zip")
                inner = outer
            scan = cs.Scan()
            cs.scan_artifact(scan, "top.zip", inner, scan.rules())
            self.assertEqual([f.kind for f in scan.found], ["너무 깊은 압축(안을 못 봄)"])


if __name__ == "__main__":
    unittest.main()
