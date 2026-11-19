#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Records how the Koha oracle resolves circulation rules (task 3.2).

    python3 -I tools/circ-rules-recorder/record.py > scenarios/circulacao/regras-efetivas.koha.json

Reads ELZEVIR_ORACLE_URL (staff base URL), ELZEVIR_ORACLE_BASIC_USER/PASSWORD (the HTTP
basic box in front of it) and ELZEVIR_ORACLE_USER/PASSWORD (a Koha staff login). Never
prints them.

The oracle's own rules are few, so the recorder adds the rules of TEMPORARY for the
recording, asks Koha's REST API for the effective rules of every combination of branch,
patron category and item type below, and removes them again, checking that the rules
are back as they were. Use it only on the disposable test instance.
"""

import base64
import http.cookiejar
import json
import os
import re
import sys
import urllib.parse
import urllib.request

KOHA_VERSION = "26.05.05.000"

# Rules added for the recording: context, then rules. They cover a branch over a category,
# a category over an item type, a blank value hiding a number, and the default of all.
TEMPORARY = [
    (("*", "*", "*"), {"issuelength": "21", "fine": "2.00"}),
    (("*", "COM", "*"), {"issuelength": "7", "fine": "0.50"}),
    (("*", "*", "DV"), {"issuelength": "3", "renewalsallowed": "1"}),
    (("MAIN", "*", "DV"), {"issuelength": "2"}),
    (("*", "COM", "DV"), {"issuelength": "5", "finedays": "2"}),
    (("MAIN", "B", "*"), {"renewalsallowed": ""}),
]
LIBRARIES = ["MAIN", "*"]
CATEGORIES = ["B", "COM", "*"]
ITEM_TYPES = ["AL", "BK", "DV", "*"]


class Oracle:
    def __init__(self):
        self.base = os.environ["ELZEVIR_ORACLE_URL"].rstrip("/")
        pair = f"{os.environ['ELZEVIR_ORACLE_BASIC_USER']}:{os.environ['ELZEVIR_ORACLE_BASIC_PASSWORD']}"
        self.auth = "Basic " + base64.b64encode(pair.encode()).decode()
        jar = http.cookiejar.CookieJar()
        self.opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))

    def open(self, path, data=None, method=None, headers=None):
        # Cloudflare refuses Python's default user agent.
        all_headers = {"Authorization": self.auth, "User-Agent": "elzevir-recorder", **(headers or {})}
        request = urllib.request.Request(self.base + path, data=data, method=method, headers=all_headers)
        return self.opener.open(request, timeout=60).read().decode()

    def log_in(self):
        page = self.open("/cgi-bin/koha/mainpage.pl")
        token = re.search(r'name="csrf_token" value="([^"]+)"', page).group(1)
        form = urllib.parse.urlencode({
            "login_op": "cud-login",
            "csrf_token": token,
            "login_userid": os.environ["ELZEVIR_ORACLE_USER"],
            "login_password": os.environ["ELZEVIR_ORACLE_PASSWORD"],
            "branch": "",
        }).encode()
        page = self.open("/cgi-bin/koha/mainpage.pl", data=form)
        self.csrf = re.search(r'<meta name="csrf-token" content="([^"]+)"', page).group(1)

    def get(self, path):
        return json.loads(self.open(path, headers={"Accept": "application/json"}))

    def put_rules(self, context, rules):
        library, category, item_type = context
        body = {"context": {"library_id": library, "patron_category_id": category, "item_type_id": item_type}, **rules}
        headers = {"Accept": "application/json", "Content-Type": "application/json", "CSRF-TOKEN": self.csrf}
        self.open("/api/v1/circulation_rules", data=json.dumps(body).encode(), method="PUT", headers=headers)


def present(rules):
    """Only the rules that are set; an empty text is a value."""
    return {name: value for name, value in rules.items() if name != "context" and value is not None}


def main():
    oracle = Oracle()
    oracle.log_in()
    before = oracle.get("/api/v1/circulation_rules?effective=false")
    try:
        for context, rules in TEMPORARY:
            oracle.put_rules(context, rules)
        raw = oracle.get("/api/v1/circulation_rules?effective=false")
        effective = []
        for library in LIBRARIES:
            for category in CATEGORIES:
                for item_type in ITEM_TYPES:
                    query = urllib.parse.urlencode({
                        "effective": "true",
                        "library_id": library,
                        "patron_category_id": category,
                        "item_type_id": item_type,
                    })
                    (rules,) = oracle.get(f"/api/v1/circulation_rules?{query}")
                    effective.append({"library": library, "category": category, "item_type": item_type, "rules": present(rules)})
    finally:
        for context, rules in TEMPORARY:
            oracle.put_rules(context, {name: None for name in rules})
        if oracle.get("/api/v1/circulation_rules?effective=false") != before:
            sys.exit("the oracle's rules did not come back as they were: check them by hand")
    json.dump({
        "koha_version": KOHA_VERSION,
        "spec": "docs/specs/circulacao/regras.md",
        "recorder": "tools/circ-rules-recorder/record.py",
        "raw": [{"context": rules["context"], "rules": present(rules)} for rules in raw],
        "effective": effective,
    }, sys.stdout, indent=1, ensure_ascii=False)
    print()


if __name__ == "__main__":
    main()
