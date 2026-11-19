#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Records how the Koha oracle answers checkouts and loan limits (task 3.4).

    python3 -I tools/checkout-recorder/record.py > scenarios/circulacao/emprestimo.koha.json

Reads the same variables as tools/circ-rules-recorder/record.py and never prints them.

The recorder adds a few circulation rules for the recording, lends Items to the sample
patron through the REST API, asks `GET /api/v1/checkouts/availability` at each step, then
returns every Item and removes the rules, checking that the oracle is back as it was.
Use it only on the disposable test instance, and only with its sample patron.
"""

import json
import os
import re
import sys
import urllib.error
import urllib.parse

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "circ-rules-recorder"))
from record import KOHA_VERSION, Oracle, present  # noqa: E402

PATRON = 5034
# Rules added for the recording: an item type with its own limit and one that may not be
# lent. A patron limit for the sample patron's branch and category is added through the
# rules page, since the REST API does not take rules without an item type.
TEMPORARY = [
    (("MAIN", "*", "DV"), {"maxissueqty": "1"}),
    (("MAIN", "*", "AL"), {"maxissueqty": "0"}),
]
# The rules the checkout reads; the replay loads only these.
CHECKOUT_RULES = {
    "maxissueqty", "maxonsiteissueqty", "patron_maxissueqty", "patron_maxonsiteissueqty",
    "issuelength", "renewalperiod", "lengthunit", "daysmode", "hardduedate", "hardduedatecompare",
}


class Recorder:
    def __init__(self, oracle):
        self.oracle = oracle
        self.lent = []
        self.checks = []

    def items(self, query, count):
        params = urllib.parse.urlencode({"q": json.dumps(query), "_per_page": count})
        return self.oracle.get("/api/v1/items?" + params)

    def available(self, item_type, count):
        found = self.items({
            "item_type_id": item_type, "withdrawn": 0, "lost_status": 0,
            "not_for_loan_status": 0, "damaged_status": 0, "checked_out_date": None,
        }, count)
        if len(found) < count:
            sys.exit(f"not enough available items of type {item_type}")
        return found

    def rules(self):
        rules = []
        for raw in self.oracle.get("/api/v1/circulation_rules?effective=false"):
            kept = {name: value for name, value in present(raw).items() if name in CHECKOUT_RULES}
            if kept:
                rules.append({"context": raw["context"], "rules": kept})
        return rules

    def lend(self, item):
        headers = {"Accept": "application/json", "Content-Type": "application/json", "CSRF-TOKEN": self.oracle.csrf}
        body = json.dumps({"item_id": item["item_id"], "patron_id": PATRON}).encode()
        path = "/api/v1/checkouts"
        try:
            self.oracle.open(path, data=body, method="POST", headers=headers)
        except urllib.error.HTTPError as error:
            if error.code != 412:
                raise
            token = self.availability(item)["confirmation_token"]
            self.oracle.open(path + "?" + urllib.parse.urlencode({"confirmation": token}), data=body, method="POST", headers=headers)
        self.lent.append(item)

    def patron_limit(self, value):
        """Sets `patron_maxissueqty` for MAIN and B, or removes it with `None`."""
        fields = {"csrf_token": self.oracle.csrf, "branch": "MAIN", "categorycode": "B"}
        if value is None:
            fields["op"] = "cud-delete-branch-cat"
        else:
            fields.update({"op": "cud-add-branch-cat", "patron_maxissueqty": value,
                           "patron_maxonsiteissueqty": "", "max_holds": ""})
        self.oracle.open("/cgi-bin/koha/admin/smart-rules.pl", data=urllib.parse.urlencode(fields).encode())

    def availability(self, item):
        query = urllib.parse.urlencode({"patron_id": PATRON, "item_id": item["item_id"]})
        return self.oracle.get(f"/api/v1/checkouts/availability?{query}")

    def check(self, name, item):
        answer = self.availability(item)
        answer.pop("confirmation_token", None)
        self.checks.append({
            "name": name,
            "rules": self.rules(),
            "loans": [describe(lent) for lent in self.lent],
            "item": describe(item),
            "answer": answer,
        })

    def return_all(self):
        page = self.oracle.open("/cgi-bin/koha/circ/returns.pl")
        token = re.search(r'name="csrf_token" value="([^"]+)"', page).group(1)
        for item in self.lent:
            form = urllib.parse.urlencode({"op": "cud-checkin", "csrf_token": token, "barcode": item["external_id"]}).encode()
            self.oracle.open("/cgi-bin/koha/circ/returns.pl", data=form)
        self.lent = []


def describe(item):
    return {
        "barcode": item["external_id"],
        "item_type": item["item_type_id"],
        "home_library": item["home_library_id"],
        "holding_library": item["holding_library_id"],
        "withdrawn": item["withdrawn"],
        "not_for_loan": item["not_for_loan_status"],
        "lost": item["lost_status"],
        "restricted": item["restricted_status"] or 0,
    }


def steps(recorder):
    books = recorder.available("BK", 5)
    dvds = recorder.available("DV", 2)
    (album,) = recorder.available("AL", 1)
    (withdrawn,) = recorder.items({"withdrawn": {">": 0}, "checked_out_date": None}, 1)
    recorder.check("sem-emprestimos", books[0])
    recorder.check("limite-zero", album)
    recorder.check("retirado", withdrawn)
    recorder.lend(dvds[0])
    recorder.check("limite-da-politica", dvds[1])
    recorder.check("politica-com-limite-proprio-nao-conta", books[0])
    recorder.check("renovacao", dvds[0])
    recorder.patron_limit("1")
    recorder.check("limite-do-leitor", books[0])
    recorder.patron_limit("6")
    for book in books[:4]:
        recorder.lend(book)
    recorder.check("limite-geral", books[4])


def main():
    oracle = Oracle()
    oracle.log_in()
    before_rules = oracle.get("/api/v1/circulation_rules?effective=false")
    if oracle.get(f"/api/v1/checkouts?patron_id={PATRON}"):
        sys.exit("the sample patron already has loans: return them first")
    recorder = Recorder(oracle)
    try:
        for context, rules in TEMPORARY:
            oracle.put_rules(context, rules)
        recorder.patron_limit("6")
        steps(recorder)
    finally:
        recorder.return_all()
        recorder.patron_limit(None)
        for context, rules in TEMPORARY:
            oracle.put_rules(context, {name: None for name in rules})
        if oracle.get(f"/api/v1/checkouts?patron_id={PATRON}"):
            sys.exit("the sample patron still has loans: return them by hand")
        if oracle.get("/api/v1/circulation_rules?effective=false") != before_rules:
            sys.exit("the oracle's rules did not come back as they were: check them by hand")
    json.dump({
        "koha_version": KOHA_VERSION,
        "spec": "docs/specs/circulacao/emprestimo.md",
        "recorder": "tools/checkout-recorder/record.py",
        "patron": {"category": "B", "library": "MAIN"},
        "checks": recorder.checks,
    }, sys.stdout, indent=1, ensure_ascii=False)
    print()


if __name__ == "__main__":
    main()
