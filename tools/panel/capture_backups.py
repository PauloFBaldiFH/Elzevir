#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Exercise catalogue selection, the destination browser and verified copies in a Linux PTY."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import tempfile
import time

from capture_dashboard import Terminal, capture


def wait_for(check, label):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        if check():
            return
        time.sleep(0.1)
    raise AssertionError(label)


def run(binary, output, root):
    env = {**os.environ, "XDG_DATA_HOME": str(root), "XDG_CONFIG_HOME": str(root / "config"),
           "TERM": "xterm-256color", "LC_ALL": "C.UTF-8", "SSH_CONNECTION": "localhost fixture"}
    for key in ("DISPLAY", "WAYLAND_DISPLAY", "NO_COLOR", "ELZEVIR_DATA_DIR", "ELZEVIR_CONFIG"):
        env.pop(key, None)
    data = root / "elzevir"
    exports = root / "arquivos"
    exports.mkdir()
    config = data / "server.toml"

    def choose_folder(path):
        term.wait_for(str(data / "backups"))
        term.click(str(data / "backups"))
        term.send(b"\x1b[200~" + os.fsencode(path) + b"\x1b[201~")
        term.click("Ir para")
        term.wait_for(str(path))
        term.read(0.4)

    def export_to(folder, *, keyboard=False):
        if keyboard:
            term.send(b"e")
        else:
            term.click("Copiar para pasta")
        choose_folder(folder)
        term.click("Copiar para pasta")
        term.wait_for("Levar a cópia")


    def cli(*args, password=None):
        return subprocess.run([binary, *map(str, args)], input=password, env=env, capture_output=True,
                              check=True, timeout=15).stdout

    cli("init", "--name", "Biblioteca Horizonte", "--password-stdin",
        password=(secrets.token_urlsafe(24) + "\n").encode())
    server = subprocess.Popen([binary, "serve", "--config", config, "--listen", "127.0.0.1:0"], env=env,
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    term = None
    try:
        wait_for(lambda: (data / "elzevir-runtime.json").is_file(), "fixture server")
        term = Terminal(binary, ["panel", "--config", config, "--lang", "pt-BR"], env)
        term.size(160, 45)
        term.wait_for("Em execução")
        term.click("Central de backups")
        term.wait_for("Cópias locais recentes")
        term.click("+ Criar backup")
        term.wait_for("Explorador de pastas")
        term.click("Cancelar")
        assert not json.loads(cli("operations", "--data-dir", data, "list"))["jobs"]
        term.click("+ Criar backup")
        term.wait_for("Explorador de pastas")
        choose_folder(exports)
        term.wait_for("Pasta vazia")
        term.click("Nova pasta")
        term.send(b"Revisao")
        term.click("Criar pasta")
        term.wait_for(str(exports / "Revisao"))
        destination = exports / "Revisao"
        (destination / "Devolucoes").mkdir()
        (destination / "Inventario").mkdir()
        (destination / "leia-me.txt").write_text("Instituição de teste.\n", encoding="utf-8")
        term.click("Atualizar")
        term.wait_for("leia-me.txt")
        term.click("leia-me.txt")
        capture(term, output, "backup-manual-160x45")
        term.size(80, 24)
        term.wait_for("Nova pasta")
        capture(term, output, "backup-manual-80x24")
        term.click("Formato e espaço")
        term.wait_for("age/X25519")
        term.click("Formato e espaço")
        term.wait_for("leia-me.txt")
        term.size(160, 45)
        term.click("+ Criar backup")
        term.wait_for("Levar a cópia")
        term.wait_for("Verificado na criação")
        wait_for(lambda: list((exports / "Revisao").glob("*.age")), "additional native archive")
        archive, = (data / "backups").glob("*.age")
        other = exports / "Revisao" / archive.name
        assert hashlib.sha256(archive.read_bytes()).digest() == hashlib.sha256(other.read_bytes()).digest()
        key = root / "recover.key"
        term.click("Exportar chave")
        term.wait_for("Destino")
        term.send(b"\x1b[200~" + os.fsencode(key) + b"\x1b[201~")
        term.click("Confirmar", last=True)
        wait_for(key.exists, "recovery key exported by the panel")
        assert key.stat().st_mode & 0o077 == 0
        cli("backup", "verify", "--input", other, "--recovery-key", key)
        term.wait_for("Exportação · Concluído")
        capture(term, output, "conclusao-160x45")
        # Publish another real archive while details are open. SCP must retain the original target.
        second = json.loads(cli("backup", "now", "--data-dir", data))["id"]
        wait_for(lambda: len(list((data / "backups").glob("*.age"))) == 2, "second archive")
        assert second not in archive.name
        term.read(2)
        term.click("Preparar SCP")
        term.wait_for("SSH")
        term.send(b"biblioteca")
        term.wait_for("scp ")
        assert any(archive.name in s for s in term.screen.display)
        capture(term, output, "transferir-scp")
        term.send(b"\x1b")
        export_to(destination)
        term.wait_for("Exportação · Falhou")
        term.wait_for("Verificado na criação")
        capture(term, output, "exportacao-falhou")
        assert archive.read_bytes() == other.read_bytes()
        term.click("Verificar arquivo")
        term.click("Confirmar", last=True)
        first_id = archive.stem.removeprefix("elzevir-")
        wait_for(lambda: any(j["task"] == {"type": "verify", "id": first_id} and j["state"] == "completed"
                             for j in json.loads(cli("operations", "--data-dir", data, "list"))["jobs"]),
                 "selected archive reverified")
        term.wait_for("Nova verificação · Concluído")
        term.click("Voltar à central")
        term.wait_for("Cópias locais recentes")
        # Select the older of two rows, after a refresh of the catalogue.
        term.wait_for(second)
        first_row = term.point("Backup verificado", last=True)
        term.mouse(0, first_row)
        term.mouse(0, first_row, release=True)
        term.wait_for(archive.name)
        term.size(120, 30)
        capture(term, output, "central-120x30")
        term.size(80, 24)
        capture(term, output, "central-80x24")
        term.size(160, 45)
        capture(term, output, "central-160x45")
        term.click("Histórico")
        term.wait_for("Solicitações e etapas")
        capture(term, output, "historico-160x45")
        term.click("Central de backups")
        term.click("Ver detalhes", last=True)
        term.wait_for(archive.name)
        cli("stop", "--data-dir", data)
        server.wait(timeout=10)
        term.read(2)
        export_to(exports, keyboard=True)
        wait_for(lambda: (exports / archive.name).exists(), "offline export")
        assert (exports / archive.name).read_bytes() == archive.read_bytes()
        term.wait_for("Exportação · Concluído")
        print("PTY OK: cancellation, compact view, two archives, selected ID via SCP and verification, "
              "collision without overwrite, recovery key, offline export, history and three sizes.")

    except AssertionError:
        # Only public fixture identifiers and outcomes are useful for diagnosing the failed step.
        for receipt in (data / "host-jobs").glob("*.json"):
            event = json.loads(receipt.read_text())
            print(event.get("action"), event.get("state"), event.get("artifact_id"), flush=True)
        print("Archives:", [str(p.relative_to(root)) for p in root.rglob("*.age")], flush=True)
        if term is not None:
            term.read(0.3)
            print("\n".join(line.rstrip() for line in term.screen.display), flush=True)
        raise
    finally:
        if term is not None:
            term.close()
        if server.poll() is None:
            try:
                cli("stop", "--data-dir", data)
            finally:
                server.wait(timeout=10)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/elzevir"))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="elzevir-backup-") as tmp:
        # Background host jobs re-execute the panel binary. Keep a stable path if Cargo rebuilds it.
        binary = Path(tmp) / "bin" / "elzevir"
        binary.parent.mkdir()
        shutil.copy2(args.binary.resolve(), binary)
        run(binary, args.output, Path(tmp))
