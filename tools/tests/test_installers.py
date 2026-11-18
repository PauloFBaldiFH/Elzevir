# SPDX-License-Identifier: GPL-3.0-or-later
"""Bootstrap security checks with local fake downloads; never install host services."""
import hashlib
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class InstallerTests(unittest.TestCase):
    def test_plan_and_invalid_arguments_do_not_download(self):
        result = subprocess.run(["bash", str(ROOT / "install.sh"), "--dry-run", "--version", "v1.2.3"], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("firewall=false", result.stdout)
        for argument in ["../institution", "library;command"]:
            result = subprocess.run(["bash", str(ROOT / "install.sh"), "--dry-run", "--id", argument], capture_output=True)
            self.assertNotEqual(result.returncode, 0)

    def test_additional_institutions_have_independent_executables(self):
        plans = []
        for institution in ["library", "alpha", "beta"]:
            result = subprocess.run(["bash", str(ROOT / "install.sh"), "--dry-run", "--id", institution], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            plans.append(dict(line.split("=", 1) for line in result.stdout.splitlines())["binary"])
        self.assertEqual(len(set(plans)), 3)

    def download(self, *, bad_hash=False, symlink=False, quick_install=False, latest=None):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            fixture = directory / "fixture"
            fixture.mkdir()
            target = "x86_64" if os.uname().machine == "x86_64" else "aarch64"
            tag = (latest or "v0.1.0-alpha.1") if quick_install else "v1.2.3"
            name = f"elzevir-{tag}-{target}-unknown-linux-musl.tar.gz"
            binary = b"fictional executable; never run"
            if quick_install:
                libc = os.confstr("CS_GNU_LIBC_VERSION") or ""
                if target == "x86_64" and libc.startswith("glibc ") and tuple(map(int, libc.split()[1].split("."))) >= (2, 35):
                    name = "elzevir-linux-x86_64.tar.gz"
                binary = f"""#!/usr/bin/env bash
case "$*" in
    --version) printf '%s\\n' 'elzevir {tag[1:]}' ;;
    'panel --help') printf '%s\\n' 'Usage: elzevir panel' ;;
    *) exit 1 ;;
esac
""".encode()
            package = fixture / name
            with tarfile.open(package, "w:gz") as archive:
                for filename in ["elzevir", "README.md", "LICENSE"]:
                    header = tarfile.TarInfo(filename)
                    content = binary if filename == "elzevir" else b"fictional documentation"
                    header.size = len(content)
                    if symlink and filename == "elzevir":
                        header.type = tarfile.SYMTYPE
                        header.linkname = "/etc/passwd"
                        header.size = 0
                    archive.addfile(header, io.BytesIO(content))
            digest = "0" * 64 if bad_hash else hashlib.sha256(package.read_bytes()).hexdigest()
            (fixture / "SHA256SUMS").write_text(f"{digest}  {name}\n")
            mock = directory / "bin"
            mock.mkdir()
            curl = mock / "curl"
            curl.write_text("""#!/usr/bin/env python3
import os, shutil, sys
from pathlib import Path
args = sys.argv[1:]
fixture = Path(os.environ['ELZEVIR_TEST_FIXTURE'])
destination = args[args.index('--output') + 1]
url = args[-1]
if url.endswith('/releases/latest'):
    latest = os.environ['ELZEVIR_TEST_LATEST']
    print(url.removesuffix('/latest') + (f'/tag/{latest}' if latest else ''), end='')
else:
    assert url.split('/')[-2] == os.environ['ELZEVIR_TEST_TAG'], url
    shutil.copyfile(fixture / url.split('/')[-1], destination)
""")
            curl.chmod(0o700)
            environment = os.environ.copy()
            environment["PATH"] = str(mock) + os.pathsep + environment["PATH"]
            environment["ELZEVIR_TEST_FIXTURE"] = str(fixture)
            environment["ELZEVIR_TEST_LATEST"] = latest or ""
            environment["ELZEVIR_TEST_TAG"] = tag
            if quick_install:
                test_home = directory / "profile"
                test_home.mkdir()
                (test_home / ".bash_profile").write_text("export EXISTING_PROFILE=preserved\n")
                environment["HOME"] = str(test_home)
                environment["XDG_DATA_HOME"] = str(directory / "local data")
                script = (ROOT / "install.sh").read_text()
                data = directory / "local data" / "elzevir"
                for _ in range(2):
                    result = subprocess.run(["bash"], input=script, env=environment, capture_output=True, text=True)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn(f"elzevir {tag[1:]}", result.stdout)
                    self.assertTrue(data.is_dir())
                    sentinel = data / "preserve"
                    if sentinel.exists():
                        self.assertEqual(sentinel.read_text(), "existing data")
                    else:
                        sentinel.write_text("existing data")
                self.assertEqual((test_home / ".local/bin/elzevir").read_bytes(), binary)
                profile = (test_home / ".bash_profile").read_text()
                self.assertIn("export EXISTING_PROFILE=preserved", profile)
                self.assertEqual(profile.count("# Elzevir PATH"), 1)
                self.assertEqual((test_home / ".bashrc").read_text().count("# Elzevir PATH"), 1)
                return
            output = directory / "verified"
            result = subprocess.run(["bash", str(ROOT / "install.sh"), "--id", "library", "--version", tag, "--download-only", str(output)], env=environment, capture_output=True, text=True)
            if bad_hash or symlink:
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(output.exists(), result.stdout)
            else:
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual((output / "elzevir").read_bytes(), b"fictional executable; never run")

    def test_download_verifies_and_extracts_only_the_expected_files(self):
        self.download()

    def test_wrong_hash_cannot_reach_extraction(self):
        self.download(bad_hash=True)

    def test_matching_hash_does_not_authorize_archive_symlinks(self):
        self.download(symlink=True)

    def test_quick_install_from_stdin_falls_back_to_the_initial_alpha(self):
        self.download(quick_install=True)

    def test_quick_install_prefers_the_stable_latest_release(self):
        self.download(quick_install=True, latest="v2.0.0")


if __name__ == "__main__":
    unittest.main()
