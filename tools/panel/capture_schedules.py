#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Exercise local policy editing against a live service and capture its terminal UI."""
import argparse
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile

from capture_backups import wait_for
from capture_dashboard import Terminal, capture


def exercise(binary, output, root, baseline, catalogue_program):
    env = dict(os.environ, XDG_DATA_HOME=str(root), XDG_CONFIG_HOME=str(root / "config"),
               TERM="xterm-256color", SSH_CONNECTION="localhost fixture", LC_ALL="C.UTF-8")
    for key in ("DISPLAY", "WAYLAND_DISPLAY", "NO_COLOR", "ELZEVIR_CONFIG", "ELZEVIR_DATA_DIR"):
        env.pop(key, None)
    data = root / "elzevir"
    config = data / "server.toml"

    def cli(*args, stdin=None):
        return subprocess.run([binary, *map(str, args)], input=stdin, capture_output=True,
                              check=True, timeout=15, env=env).stdout

    def state():
        return json.loads(cli("operations", "--data-dir", data, "list"))

    cli("init", "--name", "Biblioteca Horizonte", "--password-stdin",
        stdin=(secrets.token_urlsafe(24) + "\n").encode())
    server = subprocess.Popen([binary, "serve", "--config", config, "--listen", "127.0.0.1:0"],
                              env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    term = None
    try:
        wait_for(lambda: (data / "elzevir-runtime.json").exists(), "server started")
        initial = state()["policy"]
        term = Terminal(binary, ["panel", "--config", config, "--lang", "pt-BR"], env)
        term.size(160, 45)
        term.wait_for("Em execução")
        term.click("Central de backups")
        term.click("Agendamentos", last=True)
        if baseline:
            term.wait_for("Cópias a manter")
            capture(term, output, "antes-agendamentos")
            term.click("Cancelar", last=True)
            term.click("Nuvem")
            term.wait_for("Conexões")
            capture(term, output, "antes-nuvem")
            return
        term.wait_for("Nenhum plano de nuvem")
        assert state()["policy"] == initial
        term.click("Editar agenda local")
        term.click("Cancelar", last=True)
        assert state()["policy"] == initial
        term.size(80, 24)
        term.click("Editar agenda local")
        term.click("Horário diário")
        term.send(b"23:59")
        term.click("Fuso IANA")
        term.send(b"\x7f" * 3 + b"America/Sao_Paulo")
        term.click("Confirmar", last=True)
        wait_for(lambda: state()["policy"].get("daily") == {
            "hour": 23, "minute": 59, "time_zone": "America/Sao_Paulo"}, "policy committed")
        term.wait_for("23:59 America/Sao_Paulo")
        for width, height in ((80, 24), (100, 30), (120, 30), (160, 45), (180, 50), (200, 60)):
            term.size(width, height)
            assert term.contains("Editar agenda local") and term.contains("23:59 America/Sao_Paulo")
            if width in (80, 160):
                capture(term, output, f"agendamentos-{width}x{height}")
        term.send(b"\x1b")
        term.wait_for("Cópias locais recentes")
        term.click("Agendamentos", last=True)
        term.click("Nuvem", last=True)
        term.wait_for("Consultar provedores")
        term.size(160, 45)
        capture(term, output, "nuvem-160x45")
        term.click("Planos")
        term.wait_for("Novo plano")
        term.click("Na nuvem")
        term.wait_for("Revisar download")
        term.click("Conexões")
        term.size(80, 24)
        capture(term, output, "nuvem-80x24")
        jobs_before = len(state()["jobs"])
        term.click("Consultar provedores")
        term.wait_for("Atualizar provedores")
        term.click("Cancelar", last=True)
        assert len(state()["jobs"]) == jobs_before
        term.click("Consultar provedores")
        term.click("Confirmar", last=True)

        def discovery():
            return next((j for j in reversed(state()["jobs"])
                         if j["task"]["type"] == "configure_cloud"
                         and j["state"] in ("failed", "completed")), None)

        wait_for(discovery, "real provider discovery receipt")
        discovery_state = discovery()["state"]
        if discovery_state == "failed":
            term.click("Histórico", last=True)
            term.wait_for("Falhou")
            capture(term, output, "nuvem-falha-consulta")
        if catalogue_program and discovery_state == "failed":
            # Exercise reopening a previously cached public catalogue in this disposable institution.
            # The separate native-runner test covers actual discovery, configuration and file transfer.
            catalog = subprocess.run([catalogue_program, "--config", os.devnull, "config", "providers"],
                                     check=True, capture_output=True, timeout=15).stdout
            assert len(catalog) <= 4 * 1024 * 1024
            assert any(p["Name"] == "drive" for p in json.loads(catalog))
            (data / "cloud-providers.json").write_bytes(catalog)
            term.click("Conexões")
            wait_for(lambda: (term.read(0.2) or not term.contains("- Nova conexão")), "catalogue observed")
        if discovery_state == "completed" or catalogue_program:
            jobs_before = len(state()["jobs"])
            term.size(160, 45)
            term.click("Nova conexão")
            term.wait_for("Escolher provedor")
            term.send(b"Google Drive")
            term.wait_for("drive  ·  Google Drive")
            capture(term, output, "provedor-filtro")
            term.click("drive  ·  Google Drive")
            term.wait_for("Configurar uma conexão")
            term.wait_for("drive")
            capture(term, output, "provedor-revisao")
            term.click("Cancelar", last=True)
            assert len(state()["jobs"]) == jobs_before
        term.click("Agendamentos", last=True)
        cli("stop", "--data-dir", data)
        server.wait(timeout=10)
        term.wait_for("Agenda local indisponível")
        term.click("Editar agenda local")
        term.send(b"s")
        assert not term.contains("Operação com confirmação")
        print("Cloud discovery:", discovery_state)
        print("PTY OK: policy read, cancelled, written by mouse, read back by IPC, six sizes and offline guard.")
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
    parser.add_argument("--baseline", action="store_true")
    parser.add_argument("--catalogue-program", type=Path,
                        help="Use rclone's real public provider catalogue to exercise an existing cache.")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="elzevir-agenda-") as tmp:
        exercise(args.binary.resolve(), args.output, Path(tmp), args.baseline, args.catalogue_program)
