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

    def download(self, *, bad_hash=False, symlink=False):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            fixture = directory / "fixture"
            fixture.mkdir()
            machine = os.uname().machine
            target = "x86_64" if machine == "x86_64" else "aarch64"
            name = f"elzevir-v1.2.3-{target}-unknown-linux-musl.tar.gz"
            package = fixture / name
            with tarfile.open(package, "w:gz") as archive:
                for filename in ["elzevir", "README.md", "LICENSE"]:
                    header = tarfile.TarInfo(filename)
                    content = b"fictional executable; never run"
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
            curl.write_text("#!/usr/bin/env python3\nimport os,shutil,sys\nfrom pathlib import Path\nargs=sys.argv[1:]\ndestination=args[args.index('--output')+1]\nshutil.copyfile(Path(os.environ['ELZEVIR_TEST_FIXTURE'])/args[-1].split('/')[-1],destination)\n")
            curl.chmod(0o700)
            environment = os.environ.copy()
            environment["PATH"] = str(mock) + os.pathsep + environment["PATH"]
            environment["ELZEVIR_TEST_FIXTURE"] = str(fixture)
            output = directory / "verified"
            result = subprocess.run(["bash", str(ROOT / "install.sh"), "--version", "v1.2.3", "--download-only", str(output)], env=environment, capture_output=True, text=True)
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


if __name__ == "__main__":
    unittest.main()
