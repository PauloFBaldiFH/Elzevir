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
            result = subprocess.run(["bash", str(ROOT / "install.sh"), "--dry-run", "--version", "v1.2.3", "--id", institution], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            plans.append(dict(line.split("=", 1) for line in result.stdout.splitlines())["binary"])
        self.assertEqual(len(set(plans)), 3)

    def test_linux_release_urls(self):
        with tempfile.TemporaryDirectory() as tmp:
            mock = Path(tmp)
            for arch, libc, archive in [
                ("x86_64", "glibc 2.35", "elzevir-linux-x86_64.tar.gz"),
                ("x86_64", "glibc 2.34", "elzevir-v1.2.3-x86_64-unknown-linux-musl.tar.gz"),
                ("aarch64", "glibc 2.35", "elzevir-v1.2.3-aarch64-unknown-linux-musl.tar.gz"),
            ]:
                for command, arg, yes, no in [
                    ("uname", "-m", arch, "Linux"),
                    ("getconf", "LONG_BIT", "64", libc),
                ]:
                    stub = mock / command
                    stub.write_text(f'#!/bin/sh\nif [ "$1" = "{arg}" ]; then echo "{yes}"; else echo "{no}"; fi\n')
                    stub.chmod(0o700)
                plan = subprocess.check_output(
                    ["bash", str(ROOT / "install.sh"), "--dry-run", "--version", "v1.2.3"],
                    env={**os.environ, "PATH": tmp + os.pathsep + os.environ["PATH"]}, text=True,
                ).splitlines()
                base = "https://github.com/PauloFBaldiFH/Elzevir/releases/download/v1.2.3"
                self.assertIn(f"asset_url={base}/{archive}", plan)
                self.assertIn(f"checksums_url={base}/SHA256SUMS", plan)

    def test_feed_resolves_latest_when_release_requests_fail(self):
        with tempfile.TemporaryDirectory() as tmp:
            curl = Path(tmp) / "curl"
            curl.write_text("""#!/usr/bin/env bash
case ${!#} in
    */releases.atom) echo '<id>tag:github.com,2008:Repository/123/v1.2.3-beta.1</id>' ;;
    *) exit 22 ;;
esac
""")
            curl.chmod(0o700)
            result = subprocess.run(
                ["bash", str(ROOT / "install.sh"), "--dry-run"],
                env={**os.environ, "PATH": tmp + os.pathsep + os.environ["PATH"]},
                capture_output=True, text=True, check=True,
            )
            self.assertIn("version=v1.2.3-beta.1", result.stdout.splitlines())
            self.assertIn("request failed: https://github.com/PauloFBaldiFH/Elzevir/releases/latest", result.stderr)

    def download(self, *, bad_hash=False, symlink=False, quick_install=False, latest=None, launcher=False):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            fixture = directory / "fixture"
            fixture.mkdir()
            target = "x86_64" if os.uname().machine == "x86_64" else "aarch64"
            tag = (latest or "v0.1.0-alpha.2") if quick_install else "v1.2.3"
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
    'start --help'|'desktop --help'|'desktop install'|start) [[ -n ${{ELZEVIR_TEST_CALLS:-}} ]] || exit 1; printf '%s\\n' "$*" >>"$ELZEVIR_TEST_CALLS" ;;
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
import json, os, shutil, sys
from pathlib import Path
args = sys.argv[1:]
fixture = Path(os.environ['ELZEVIR_TEST_FIXTURE'])
destination = args[args.index('--output') + 1]
url = args[-1]
if url.endswith('/releases?per_page=10'):
    assert url.startswith('https://api.github.com/repos/'), url
    print(json.dumps([{"url": "x", "tag_name": os.environ['ELZEVIR_TEST_TAG'], "prerelease": True}], indent=2))
elif url.endswith('/releases/latest'):
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
                calls = directory / "calls"
                if launcher:
                    environment["ELZEVIR_TEST_CALLS"] = str(calls)
                    environment["DISPLAY"] = ":0"
                    environment.pop("WAYLAND_DISPLAY", None)
                for _ in range(2):
                    # A new session has no controlling terminal, like an unattended run.
                    result = subprocess.run(["bash"], input=script, env=environment, capture_output=True, text=True, start_new_session=True)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn(f"elzevir {tag[1:]}", result.stdout)
                    self.assertIn("To open the panel later" if launcher else "Next step", result.stdout)
                if launcher:
                    recorded = calls.read_text().splitlines()
                    self.assertEqual(recorded.count("desktop install"), 2)
                    self.assertNotIn("start", recorded, "the panel never opens without a terminal")
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

    def test_quick_install_without_stable_release_takes_the_newest_prerelease(self):
        self.download(quick_install=True)

    def test_quick_install_adds_the_desktop_launcher_and_skips_the_panel_without_a_terminal(self):
        self.download(quick_install=True, launcher=True)

    def test_quick_install_prefers_the_stable_latest_release(self):
        self.download(quick_install=True, latest="v2.0.0")


if __name__ == "__main__":
    unittest.main()
