#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Exercise the restore workspace and the personal CLI on a disposable institution."""
import argparse
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile

from capture_backups import wait_for
from capture_dashboard import Terminal, capture


def exercise(binary, root, output, baseline):
    data = root / "elzevir"
    config = data / "server.toml"
    env = dict(os.environ, XDG_DATA_HOME=str(root), XDG_CONFIG_HOME=str(root / "config"),
               HOME=str(root), TERM="xterm-256color", LC_ALL="C.UTF-8", SSH_CONNECTION="local fixture")
    for key in ("DISPLAY", "WAYLAND_DISPLAY", "NO_COLOR", "ELZEVIR_CONFIG", "ELZEVIR_DATA_DIR"):
        env.pop(key, None)

    def cli(*args, stdin=None):
        return subprocess.run([binary, *map(str, args)], input=stdin, env=env,
                              capture_output=True, check=True, timeout=30).stdout

    def serve():
        server = subprocess.Popen([binary, "serve", "--config", config, "--listen", "127.0.0.1:0"],
                                  env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return server

    cli("init", "--name", "Biblioteca Horizonte", "--password-stdin",
        stdin=(secrets.token_urlsafe(24) + "\n").encode())
    server = serve()
    term = None
    try:
        wait_for(lambda: (data / "elzevir-runtime.json").exists(), "test service started")
        job = json.loads(cli("backup", "now", "--config", config))["id"]
        archive = data / "backups" / f"elzevir-{job}.age"
        wait_for(lambda: archive.exists(), "encrypted file published")
        cli("backup", "verify", "--input", archive, "--recovery-key", data / "backup.key")
        term = Terminal(binary, ["panel", "--config", config, "--lang", "pt-BR"], env)
        term.size(80, 24)
        term.wait_for("Em execução")
        term.click("Central de backups")
        term.wait_for("Restaurar dados")
        term.click("Restaurar dados")
        if baseline:
            term.wait_for("Validar cópia")
            term.send(b"\x1bOQ")
            term.wait_for(archive.name)
            capture(term, output, "antes-seletor")
            term.send(b"\x1b")
            for label, path in (("Arquivo cifrado", archive), ("Arquivo da chave", data / "backup.key")):
                term.click(label, last=True)
                term.send(b"\x1b[200~" + os.fsencode(path) + b"\x1b[201~")
            term.read(1)
            term.click("Validar cópia", last=True)
            term.wait_for("Confirmar escolha")
            capture(term, output, "antes-revisao")
            return
        capture(term, output, "restauracao-inicial")
        term.click("Escolher cópia e chave")
        term.click("Escolher arquivo")
        term.wait_for(archive.name)
        capture(term, output, "escolher-arquivo-80x24")
        term.size(160, 45)
        term.wait_for(archive.name)
        capture(term, output, "escolher-arquivo-160x45")
        term.size(80, 24)
        term.click(archive.name)
        term.click("Escolher chave")
        term.wait_for("backup.key")
        capture(term, output, "seletor-chave")
        term.click("backup.key")
        term.wait_for("Arquivo regular legível")
        term.read(0.6)
        capture(term, output, "validar-arquivo-chave")
        term.click("Validar cópia", last=True)
        term.wait_for("elzevir backup restore --help")
        capture(term, output, "revisao-pessoal-80x24")
        term.click("Voltar")
        wait_for(lambda: (term.read(0.1) or not term.contains("elzevir backup restore --help")), "review closed")
        term.wait_for("Cópia validada ·")
        capture(term, output, "copia-validada-80x24")
        term.click("Revisar cópia")
        term.wait_for("elzevir backup restore --help")
        term.size(160, 45)
        capture(term, output, "revisao-pessoal")
        term.click("Voltar")
        wait_for(lambda: (term.read(0.1) or not term.contains("elzevir backup restore --help")), "review closed")
        term.wait_for("Escolher cópia e chave")
        # A mismatched, valid test identity must clear the previous review.
        wrong_key = root / "outra-chave.txt"
        wrong_key.write_bytes((Path(__file__).resolve().parents[2] / "crates/elzevir/tests/data/gzip-v1/fixture.key").read_bytes())
        term.click("Escolher cópia e chave")
        term.click("Escolher arquivo")
        term.wait_for(archive.name)
        term.click(archive.name)
        term.click("Arquivo da chave", last=True)
        term.send(b"\x1b[200~" + os.fsencode(wrong_key) + b"\x1b[201~")
        term.read(1)
        term.click("Validar cópia", last=True)
        term.wait_for("Validação falhou")
        assert term.contains("- Revisar cópia")
        capture(term, output, "chave-divergente")
        assert "policy" in json.loads(cli("operations", "--data-dir", data, "list"))
        term.close()
        term = None
        cli("stop", "--data-dir", data)
        server.wait(timeout=15)
        original = config.read_bytes()
        assert b"Biblioteca Horizonte" in original
        config.write_bytes(original.replace(b"Biblioteca Horizonte", b"Nome temporario"))
        cli("backup", "restore", "--input", archive, "--recovery-key", data / "backup.key",
            "--config", config, "--data-dir", data, "--confirm-restore")
        assert b"Biblioteca Horizonte" in config.read_bytes()
        server = serve()
        wait_for(lambda: (data / "elzevir-runtime.json").exists(), "restored service started")
        assert "policy" in json.loads(cli("operations", "--data-dir", data, "list"))
        print("PTY OK: archive and key by mouse, verification, personal review, return. "
              "Personal CLI restored the saved configuration and the restarted service answered IPC.")
    finally:
        if term:
            term.close()
        if server.poll() is None:
            try:
                cli("stop", "--data-dir", data)
            finally:
                server.wait(timeout=15)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/elzevir"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--baseline", action="store_true")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="elzevir-restauracao-") as tmp:
        exercise(args.binary.resolve(), Path(tmp), args.output, args.baseline)
