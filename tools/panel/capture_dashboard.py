#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Exercise the real Linux panel in a disposable PTY and render its terminal cells to PNG."""
import argparse
import base64
import errno
import fcntl
import json
import os
from pathlib import Path
import pty
import secrets
import select
import signal
import struct
import subprocess
import tempfile
import termios
import time
from contextlib import contextmanager

import pyte
from PIL import Image, ImageColor, ImageDraw, ImageFont


class Terminal:
    def __init__(self, binary, args, env):
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 120, 0, 0))
            os.execve(binary, [str(binary), *map(str, args)], env)
        self.screen = pyte.Screen(120, 30)
        self.stream = pyte.ByteStream(self.screen)
        self.output = bytearray()
        self.read(0.55)

    def read(self, seconds):
        deadline = time.monotonic() + seconds
        while (remaining := deadline - time.monotonic()) > 0:
            if not select.select([self.fd], [], [], remaining)[0]:
                return
            try:
                data = os.read(self.fd, 65536)
            except OSError as error:
                if error.errno != errno.EIO:
                    raise
                return
            if not data:
                return
            self.output.extend(data)
            self.stream.feed(data)

    def send(self, data):
        os.write(self.fd, data)
        self.read(0.2)

    def size(self, width, height):
        self.screen.resize(height, width)
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))
        os.kill(self.pid, signal.SIGWINCH)
        self.read(0.45)

    def contains(self, label):
        return any(label in line for line in self.screen.display)

    def wait_for(self, label):
        deadline = time.monotonic() + 12
        while not self.contains(label) and time.monotonic() < deadline:
            self.read(0.1)
        assert self.contains(label), (label, self.screen.display)

    def point(self, label, last=False):
        matches = [(line.index(label) + 2, y + 1) for y, line in enumerate(self.screen.display) if label in line]
        assert matches, (label, self.screen.display)
        return matches[-1 if last else 0]

    def mouse(self, code, point, release=False):
        x, y = point
        self.send(f"\x1b[<{code};{x};{y}{'m' if release else 'M'}".encode())

    def click(self, label, last=False):
        point = self.point(label, last)
        self.mouse(0, point)
        self.mouse(0, point, release=True)

    def close(self):
        self.send(b"\x1b")
        self.send(b"q")
        if not os.waitpid(self.pid, os.WNOHANG)[0]:
            os.kill(self.pid, signal.SIGTERM)
            os.waitpid(self.pid, 0)
        os.close(self.fd)


def color(code, fallback):
    if code == "default":
        return fallback
    if len(code) == 6 and all(c in "0123456789abcdef" for c in code):
        return "#" + code
    return ImageColor.getrgb(code)


def capture(term, folder, name):
    screen = term.screen
    image = Image.new("RGB", (10 * screen.columns, 22 * screen.lines))
    draw = ImageDraw.Draw(image)
    fonts = [ImageFont.truetype("DejaVuSansMono.ttf", 16), ImageFont.truetype("DejaVuSansMono-Bold.ttf", 16)]
    for y in range(screen.lines):
        for x in range(screen.columns):
            cell = screen.buffer[y][x]
            draw.rectangle((10*x, 22*y, 10*x+9, 22*y+21), fill=color(cell.bg, "#0b1725"))
    for y in range(screen.lines):
        for x in range(screen.columns):
            cell = screen.buffer[y][x]
            if cell.data.strip():
                draw.text((10*x, 22*y+1), cell.data, fill=color(cell.fg, "#e5eff8"), font=fonts[cell.bold])
    image.save(folder / (name + ".png"), optimize=True)


def exercise(binary, folder, root):
    env = {**os.environ, "XDG_DATA_HOME": str(root), "XDG_CONFIG_HOME": str(root / "config"),
           "TERM": "xterm-256color", "LC_ALL": "C.UTF-8"}
    for key in ["DISPLAY", "WAYLAND_DISPLAY", "NO_COLOR", "ELZEVIR_DATA_DIR", "ELZEVIR_CONFIG"]:
        env.pop(key, None)
    data = root / "elzevir"

    def cli(*args, password=None):
        return subprocess.run([binary, *map(str,args)], input=password, env=env, capture_output=True,
                              check=True, timeout=15).stdout

    @contextmanager
    def terminal(*args):
        term = Terminal(binary, args, env)
        try:
            yield term
        finally:
            term.close()

    def jobs():
        return json.loads(cli("operations", "--data-dir", data, "list"))["jobs"]

    with terminal("start", "--lang", "pt-BR") as term:
        assert term.contains("Configurar biblioteca")
        capture(term, folder, "instalacao-nova")
    assert not (data / "elzevir.db").exists()
    cli("init", "--name", "Biblioteca Horizonte", "--password-stdin",
        password=(secrets.token_urlsafe(24) + "\n").encode())
    config = data / "server.toml"
    server = subprocess.Popen([binary, "serve", "--config", config, "--listen", "127.0.0.1:0"],
                              env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(100):
            if server.poll() is not None:
                raise RuntimeError("The fixture server exited during startup")
            try:
                jobs()
                break
            except subprocess.CalledProcessError:
                time.sleep(0.1)
        else:
            raise RuntimeError("The fixture administrative channel did not become ready")
        with terminal("panel", "--config", config, "--lang", "pt-BR") as term:
            term.size(160, 45)
            term.wait_for("Em execução")
            point = term.point("+ Criar backup")
            term.mouse(0, point)
            term.mouse(32, (1, 1))
            term.mouse(0, (1, 1), release=True)
            assert not term.contains("Operação com confirmação")
            term.click("+ Criar backup")
            term.wait_for("Operação com confirmação")
            capture(term, folder, "dialogo-backup")
            # The surrounding dashboard is inert while the confirmation is visible.
            term.click("Central de backups")
            term.wait_for("Operação com confirmação")
            term.size(80, 24)
            term.click("Cancelar")
            assert not jobs()
            term.size(160, 45)
            term.click("+ Criar backup")
            term.click("+ Criar backup", last=True)
            for _ in range(100):
                if any(j["state"] == "completed" for j in jobs()):
                    break
                time.sleep(0.1)
            assert [j["state"] for j in jobs()] == ["completed"]
            archive, = (data / "backups").glob("*.age")
            key = root / "recovery.key"
            cli("backup", "key", "--data-dir", data, "--output", key)
            cli("backup", "verify", "--input", archive, "--recovery-key", key)
            term.wait_for("Verificado na criação")
            assert term.contains("Testar HTTPS")
            term.mouse(35, (1, 1))
            capture(term, folder, "dashboard-160x45")
            term.size(120, 30)
            capture(term, folder, "dashboard-120x30")
            term.size(80, 24)
            capture(term, folder, "dashboard-80x24")
            term.mouse(65, (25, 12))
            for _ in range(8):
                if term.contains("Testar acessos locais"):
                    break
                term.click("Descer")
            assert term.contains("Testar acessos locais")
            capture(term, folder, "dashboard-80x24-acesso")
            url = next(line.split("http://")[1].split()[0] for line in term.screen.display if "Catálogo local" in line)
            term.click("Copiar")
            assert b"\x1b]52;c;" + base64.b64encode(("http://" + url).encode()) + b"\x07" in term.output
            term.click("Testar acessos locais")
            term.wait_for("Catálogo OK")
            assert term.contains("Equipe OK")
        with terminal("panel", "--config", config, "--lang", "en", "--ascii") as term:
            term.size(160, 45)
            term.wait_for("Verified at creation")
            term.click("+ Create backup")
            term.wait_for("Cancel")
            term.click("Export folder")
            term.send(b"\x1b[200~" + (("a" * 160) + "é-SUFFIX").encode() + b"\x1b[201~")
            term.wait_for("SUFFIX")
            term.size(79, 17)
            term.send(b"\r")
            term.size(80, 24)
            assert term.contains("Cancel")
            term.send(b"\x1b")
            assert len(jobs()) == 1
    finally:
        try:
            cli("stop", "--data-dir", data)
        finally:
            if server.poll() is None:
                server.wait(timeout=10)

    with terminal("panel", "--demo", "--lang", "pt-BR") as term:
        term.click("Galeria de controles")
        term.click("Indisponível")
        term.click("Em andamento")
        assert not term.contains("Exemplo executado")
        term.send(b"\t")
        term.mouse(35, term.point("Ação secundária"))
        capture(term, folder, "galeria-foco-hover")
        term.send(b" ")
        assert term.contains("Exemplo executado")
        point = term.point("Simular erro")
        term.mouse(0, point)
        capture(term, folder, "galeria-pressionado")
        term.mouse(0, point, release=True)
        assert term.contains("retornou um erro")
        capture(term, folder, "galeria-retorno")
        term.send(b"\x1b[Z")
        term.send(b"\x1b")
        assert not term.contains("Clique nos exemplos")
    print("PTY OK: press/release, modal, resize, keyboard, scrolling, OSC 52, HTTP and one verified encrypted backup.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/elzevir"))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="elzevir-ux-") as directory:
        exercise(args.binary.resolve(), args.output, Path(directory))
