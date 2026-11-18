# SPDX-License-Identifier: GPL-3.0-or-later
"""Exercise an unpacked release binary from outside the checkout."""

import json
from pathlib import Path
import secrets
import subprocess
import sys
import tempfile
import time
import urllib.request


def wait_ready(binary, config, server, directory):
    deadline = time.monotonic() + 45
    while server.poll() is None and time.monotonic() < deadline:
        status = subprocess.run(
            [binary, "status", "--config", config, "--json"],
            cwd=directory, capture_output=True, text=True, timeout=5,
        )
        if status.returncode == 0:
            return json.loads(status.stdout)
        time.sleep(0.25)
    raise RuntimeError("Packaged server did not become ready")


def smoke(binary, tag):
    with tempfile.TemporaryDirectory(prefix="elzevir-release-") as directory:
        version = subprocess.check_output([binary, "--version"], cwd=directory, text=True, timeout=5)
        assert version.strip() == f"elzevir {tag.removeprefix('v')}", version
        panel = subprocess.check_output([binary, "panel", "--help"], cwd=directory, text=True, timeout=5)
        assert "--demo" in panel and "--config" in panel, panel
        config = Path(directory) / "elzevir.toml"
        data = Path(directory) / "data"
        subprocess.run(
            [binary, "init", "--name", "Release smoke test", "--config", config,
             "--data-dir", data, "--listen", "127.0.0.1:0", "--password-stdin"],
            cwd=directory, input=secrets.token_hex(24) + "\n", text=True, check=True, timeout=45,
        )
        log_path = Path(directory) / "serve.log"
        with log_path.open("wb") as log:
            server = subprocess.Popen(
                [binary, "serve", "--config", config], cwd=directory,
                stdout=log, stderr=subprocess.STDOUT,
            )
            try:
                status = wait_ready(binary, config, server, directory)
                assert status["state"] == "running", status
                assert status["health"]["database"] == status["health"]["indexer"] == "ok", status
                assert (data / "elzevir.db").is_file()
                client = urllib.request.build_opener(urllib.request.ProxyHandler({}))
                base = f"http://{status['listen']}"
                with client.open(f"{base}/", timeout=5) as page:
                    assert b'name="q"' in page.read()
                with client.open(f"{base}/search?q=release-smoke", timeout=5) as page:
                    assert b"release-smoke" in page.read()
                with client.open(f"{base}/static/elzevir.css", timeout=5) as css:
                    assert css.headers.get_content_type() == "text/css" and css.read()
            except Exception:
                print(log_path.read_text(encoding="utf-8", errors="replace"), file=sys.stderr)
                raise
            finally:
                server.terminate()
                try:
                    server.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()
    print("Packaged CLI, panel, SQLite, Tantivy, catalog and web assets verified.")


if __name__ == "__main__":
    smoke(Path(sys.argv[1]).resolve(strict=True), sys.argv[2])
