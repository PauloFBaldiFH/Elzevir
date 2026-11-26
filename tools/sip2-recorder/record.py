#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Records how the Koha oracle's SIP2 server answers a self-check terminal (task 4.5b).

Run it on the test VM itself, where Koha's SIP server listens:

    python3 -I tools/sip2-recorder/record.py > sip2.koha.json

Reads, and never prints:

    ELZEVIR_SIP_LOGIN, ELZEVIR_SIP_PASSWORD    the SIP account of SIPconfig.xml
    ELZEVIR_SIP_PATRON                         the sample patron's card number
    ELZEVIR_SIP_PATRON_PASSWORD                the sample patron's OPAC password
    ELZEVIR_SIP_ITEM                           the barcode of an Item that can be lent
                                               to the sample patron, not on loan now
    ELZEVIR_SIP_HOST, ELZEVIR_SIP_PORT         default 127.0.0.1 and 6001
    ELZEVIR_SIP_INSTITUTION                    default MAIN

The conversation lends the Item to the sample patron, renews it and takes it back, so
the Item ends where it started; it also asks with a wrong password and unknown barcodes.
Use it only on the disposable test instance, and only with its sample patron.

The output keeps every response as Koha wrote it, except the patron's personal fields
(name, address, e-mail, phone, birth date), which become NAME, ADDRESS and so on, and
the card, logins, passwords and barcode of the run, which become placeholders in requests
and responses alike, field by field. Checksums are kept as Koha computed them, so they no
longer match the masked text. Check the file before sending it.
"""

import datetime
import json
import os
import re
import socket
import sys

KOHA_VERSION = "26.05.05.000"
# Personal fields of patron responses, replaced by a placeholder.
PERSONAL = {"AE": "NAME", "BD": "ADDRESS", "BE": "EMAIL", "BF": "PHONE", "PB": "BIRTH", "DA": "NAME"}
WRONG_PASSWORD = "not-the-password"
UNKNOWN_BARCODE = "ELZEVIR-NO-SUCH-ITEM"


def checksum(text):
    """The four hex digits that make the bytes sum to zero (SIP2 error detection)."""
    return f"{(-sum(text.encode('latin-1', 'replace'))) & 0xFFFF:04X}"


class Terminal:
    def __init__(self, host, port):
        self.socket = socket.create_connection((host, port), timeout=30)
        self.buffer = b""
        self.sequence = 0

    def ask(self, message):
        """Sends one message with error detection and returns the response, without its
        terminator."""
        body = f"{message}AY{self.sequence}AZ"
        self.sequence = (self.sequence + 1) % 10
        self.socket.sendall((body + checksum(body) + "\r").encode("utf-8"))
        while b"\r" not in self.buffer:
            chunk = self.socket.recv(4096)
            if not chunk:
                raise SystemExit(f"the server closed the connection after {message[:2]}")
            self.buffer += chunk
        response, _, self.buffer = self.buffer.partition(b"\r")
        self.buffer = self.buffer.lstrip(b"\n")
        return response.decode("utf-8", "replace")


def now():
    return datetime.datetime.now().strftime("%Y%m%d    %H%M%S")


# A field starts after a `|`, or right after the fixed fields: a time, or the login's
# two algorithm digits.
FIELD_START = r"(?:(?<=\|)|(?<=\d{6})|(?<=^9300))"


class Recording:
    """Keeps each exchange with the run's secrets and the patron's personal fields replaced
    by placeholders. Values are replaced by field, never as loose substrings, so that a
    short card or login cannot change other fields or the placeholders themselves."""

    def __init__(self, env):
        # Field, the run's value, placeholder: only that exact value is replaced, so the
        # unknown card and barcode asked on purpose stay readable.
        self.secrets = [
            ("CN", env["login"], "LOGIN"),
            ("CO", env["password"], "PASSWORD"),
            ("AA", env["patron"], "PATRON"),
            ("CY", env["patron"], "PATRON"),
            ("AD", env["patron_password"], "PATRON-PASSWORD"),
            ("AB", env["item"], "ITEM"),
        ]
        self.names = set()
        self.exchanges = []

    def hide(self, text):
        for field, value, placeholder in self.secrets:
            if value:
                text = re.sub(rf"{FIELD_START}{field}{re.escape(value)}(?=\|)", field + placeholder, text)
        for field, placeholder in PERSONAL.items():
            text = re.sub(rf"{FIELD_START}{field}[^|]+", field + placeholder, text)
        # The patron's name inside screen messages ("Item not checked out to <name>").
        for name in self.names:
            text = text.replace(name, "NAME")
        return text

    def add(self, name, request, response):
        for found in re.finditer(rf"{FIELD_START}(?:AE|DA)([^|]+)", response):
            self.names.add(found.group(1))
        self.exchanges.append({
            "name": name,
            "request": self.hide(request),
            "response": self.hide(response),
        })


def converse(terminal, recording, env):
    institution = env["institution"]
    patron, password, item = env["patron"], env["patron_password"], env["item"]

    def ask(name, message):
        response = terminal.ask(message)
        recording.add(name, message, response)
        return response

    # With error detection, Koha answers `941AY<n>AZ<checksum>`.
    if not ask("login", f"9300CN{env['login']}|CO{env['password']}|CP{institution}|").startswith("941"):
        raise SystemExit("the SIP login was refused: check the account in SIPconfig.xml")
    ask("sc_status", "9900302.00")
    ask("patron_status", f"23001{now()}AO{institution}|AA{patron}|AC|AD{password}|")
    ask("patron_status_wrong_password", f"23001{now()}AO{institution}|AA{patron}|AC|AD{WRONG_PASSWORD}|")
    ask("patron_status_unknown", f"23001{now()}AO{institution}|AA{UNKNOWN_BARCODE}|AC|AD{password}|")
    ask("patron_information", f"63001{now()}          AO{institution}|AA{patron}|AD{password}|")
    ask("item_information", f"17{now()}AO{institution}|AB{item}|AC|")
    ask("item_information_unknown", f"17{now()}AO{institution}|AB{UNKNOWN_BARCODE}|AC|")
    checkout = ask("checkout", f"11YN{now()}{' ' * 18}AO{institution}|AA{patron}|AB{item}|AC|AD{password}|BON|BIN|")
    lent = checkout.startswith("121")
    ask("checkout_unknown_item", f"11YN{now()}{' ' * 18}AO{institution}|AA{patron}|AB{UNKNOWN_BARCODE}|AC|AD{password}|BON|BIN|")
    ask("checkout_again", f"11YN{now()}{' ' * 18}AO{institution}|AA{patron}|AB{item}|AC|AD{password}|BON|BIN|")
    ask("item_information_lent", f"17{now()}AO{institution}|AB{item}|AC|")
    for summary in range(6):
        flags = [" "] * 10
        flags[summary] = "Y"
        ask(f"patron_information_summary_{summary}",
            f"63001{now()}{''.join(flags)}AO{institution}|AA{patron}|AD{password}|BP1|BQ5|")
    ask("renew", f"29NN{now()}{' ' * 18}AO{institution}|AA{patron}|AD{password}|AB{item}|AC|BON|")
    ask("renew_unknown_item", f"29NN{now()}{' ' * 18}AO{institution}|AA{patron}|AD{password}|AB{UNKNOWN_BARCODE}|AC|BON|")
    ask("checkin", f"09N{now()}{now()}AP{institution}|AO{institution}|AB{item}|AC|BIN|")
    ask("checkin_not_lent", f"09N{now()}{now()}AP{institution}|AO{institution}|AB{item}|AC|BIN|")
    ask("checkin_unknown_item", f"09N{now()}{now()}AP{institution}|AO{institution}|AB{UNKNOWN_BARCODE}|AC|BIN|")
    ask("item_information_returned", f"17{now()}AO{institution}|AB{item}|AC|")
    ask("end_patron_session", f"35{now()}AO{institution}|AA{patron}|AC|AD{password}|")
    ask("resend", "97")
    return lent


def main():
    try:
        env = {
            "login": os.environ["ELZEVIR_SIP_LOGIN"],
            "password": os.environ["ELZEVIR_SIP_PASSWORD"],
            "patron": os.environ["ELZEVIR_SIP_PATRON"],
            "patron_password": os.environ["ELZEVIR_SIP_PATRON_PASSWORD"],
            "item": os.environ["ELZEVIR_SIP_ITEM"],
        }
    except KeyError as missing:
        raise SystemExit(f"missing variable {missing}") from None
    env["institution"] = os.environ.get("ELZEVIR_SIP_INSTITUTION", "MAIN")
    host = os.environ.get("ELZEVIR_SIP_HOST", "127.0.0.1")
    port = int(os.environ.get("ELZEVIR_SIP_PORT", "6001"))
    recording = Recording(env)
    terminal = Terminal(host, port)
    lent = converse(terminal, recording, env)
    json.dump({
        "koha": KOHA_VERSION,
        "institution": env["institution"],
        "lent": lent,
        "exchanges": recording.exchanges,
    }, sys.stdout, ensure_ascii=False, indent=2)
    sys.stdout.write("\n")
    if not lent:
        print("warning: the checkout was refused, so renew and checkin recorded refusals; "
              "choose an Item the sample patron can borrow", file=sys.stderr)


if __name__ == "__main__":
    main()
