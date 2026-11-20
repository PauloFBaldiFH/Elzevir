#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Records how the Koha oracle fines late returns and settles them (task 3.7).

    python3 -I tools/fines-recorder/record.py > scenarios/circulacao/multa-conta.koha.json

Reads the same variables as tools/circ-rules-recorder/record.py and never prints them.

The recorder turns `finesMode` to `production` and adds a fine rule for the recording,
lends two Items to the sample patron, returns them late on different days
(`SpecifyReturnDate`), asks whether a third Item may be lent, pays part of the debt
without choosing lines, and writes off the rest of the second fine. Afterwards it puts the
rules and the preference back and checks that the patron owes what they owed before and has
no loans. The account lines stay in the patron's history. Use it only on the disposable
test instance, and only with its sample patron.
"""

import datetime
import json
import os
import sys
import urllib.parse

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "circ-rules-recorder"))
from record import KOHA_VERSION, Oracle  # noqa: E402

import importlib.util  # noqa: E402

_spec = importlib.util.spec_from_file_location(
    "checkin_record", os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "checkin-recorder", "record.py"))
checkin = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(checkin)

PATRON = checkin.PATRON
CONTEXT = checkin.CONTEXT
# Half a unit a day from the first day late, and no suspension.
TEMPORARY = {"fine": "0.50", "chargeperiod": "1", "firstremind": "0", "finedays": "0"}
# The preferences the replay depends on; the recording fails if one is not Koha's default.
PREFERENCES = {
    "CircControl": "ItemHomeLibrary",
    "HomeOrHoldingBranch": "holdingbranch",
    "useDaysMode": "Calendar",
    "finesCalendar": "noFinesWhenClosed",
    "FinesIncludeGracePeriod": "1",
    "CalculateFinesOnReturn": "1",
    "MaxFine": "",
    "noissuescharge": "5",
    "AllowFineOverride": "0",
    "AllFinesNeedOverride": "1",
    "AccountAutoReconcile": "0",
    "RequirePaymentType": "0",
    "SpecifyReturnDate": "1",
}
FINES_MODE = "finesMode"
RULES = checkin.RULES | {"fine", "chargeperiod", "chargeperiod_charge_at", "overduefinescap",
                         "cap_fine_to_replacement_price", "recall_overdue_fine"}
DAYS_LATE = (4, 7)
PAID = "3.00"


def line(raw):
    """What the replay compares of an account line."""
    return {name: raw.get(name) for name in
            ("account_line_id", "debit_type", "credit_type", "status", "amount", "amount_outstanding", "item_id")}


class Recorder(checkin.Recorder):
    def rules(self):
        rules = []
        for raw in self.oracle.get("/api/v1/circulation_rules?effective=false"):
            kept = {name: value for name, value in checkin.present(raw).items() if name in RULES}
            if kept:
                rules.append({"context": raw["context"], "rules": kept})
        return rules

    def account(self):
        base = f"/api/v1/patrons/{PATRON}/account"
        return {
            "debits": sorted((line(raw) for raw in self.oracle.get(base + "/debits?_per_page=-1")),
                             key=lambda found: found["account_line_id"]),
            "credits": sorted((line(raw) for raw in self.oracle.get(base + "/credits?_per_page=-1")),
                              key=lambda found: found["account_line_id"]),
        }

    def credit(self, kind, amount, lines=None):
        body = {"credit_type": kind, "amount": float(amount)}
        if lines:
            body["account_lines_ids"] = lines
        return json.loads(self.oracle.open(f"/api/v1/patrons/{PATRON}/account/credits",
                                           data=json.dumps(body).encode(), method="POST",
                                           headers=self.json_headers))["account_line_id"]


def steps(recorder):
    first, second, third = recorder.available(3)
    before = {found["account_line_id"] for found in recorder.account()["debits"]}
    loans = [recorder.lend(item) for item in (first, second)]
    current = recorder.loans(False)
    record = {
        "items": [checkin.describe(item) for item in (first, second, third)],
        "rules": recorder.rules(),
        "calendar": recorder.calendar("MAIN"),
        "checkouts": [],
    }
    for item, loan, late in zip((first, second), loans, DAYS_LATE):
        found = checkin.loan_of(current, loan)
        due = datetime.datetime.fromisoformat(found["due_date"])
        back = due.replace(hour=10, minute=0, second=0) + datetime.timedelta(days=late)
        recorder.give_back(item, back)
        returned = checkin.loan_of(recorder.loans(True), loan)
        record["checkouts"].append({"at": found["checkout_date"], "due": found["due_date"],
                                    "returned": returned["checkin_date"], "library": found["library_id"]})
    fines = [found for found in recorder.account()["debits"] if found["account_line_id"] not in before]
    record["fines"] = fines
    record["debt"] = recorder.availability(third)
    record["payment"] = {"amount": PAID, "credit": recorder.credit("PAYMENT", PAID)}
    record["after_payment"] = [found for found in recorder.account()["debits"] if found["account_line_id"] not in before]
    left = next(found for found in record["after_payment"] if found["account_line_id"] == fines[-1]["account_line_id"])
    amount = left["amount_outstanding"]
    record["writeoff"] = {"amount": amount, "lines": [left["account_line_id"]],
                          "credit": recorder.credit("WRITEOFF", amount, [left["account_line_id"]])}
    record["after_writeoff"] = [found for found in recorder.account()["debits"] if found["account_line_id"] not in before]
    record["credits"] = [found for found in recorder.account()["credits"]
                         if found["account_line_id"] in (record["payment"]["credit"], record["writeoff"]["credit"])]
    return record


def main():
    oracle = Oracle()
    oracle.log_in()
    recorder = Recorder(oracle)
    for name, value in PREFERENCES.items():
        if recorder.preference(name) != value:
            sys.exit(f"{name} is not {value!r} on the oracle")
    before_rules = oracle.get("/api/v1/circulation_rules?effective=false")
    before_mode = recorder.preference(FINES_MODE)
    before_balance = oracle.get(f"/api/v1/patrons/{PATRON}/account")["balance"]
    if before_balance:
        sys.exit("the sample patron owes something: settle it first")
    if recorder.loans(False):
        sys.exit("the sample patron already has loans: return them first")
    if oracle.get(f"/api/v1/patrons/{PATRON}")["restricted"]:
        sys.exit("the sample patron is restricted: lift it first")
    try:
        recorder.set_preference(FINES_MODE, "production")
        oracle.put_rules(CONTEXT, TEMPORARY)
        record = steps(recorder)
    finally:
        for loan in recorder.loans(False):
            item = oracle.get(f"/api/v1/items/{loan['item_id']}")
            recorder.give_back(item, datetime.datetime.now())
        balance = oracle.get(f"/api/v1/patrons/{PATRON}/account")["balance"]
        if balance > 0:
            recorder.credit("WRITEOFF", balance)
        recorder.lift_suspensions()
        oracle.put_rules(CONTEXT, {name: None for name in TEMPORARY})
        recorder.set_preference(FINES_MODE, before_mode)
        if recorder.loans(False):
            sys.exit("the sample patron still has loans: return them by hand")
        if oracle.get(f"/api/v1/patrons/{PATRON}/account")["balance"] != before_balance:
            sys.exit("the sample patron's balance changed: check the account by hand")
        if oracle.get("/api/v1/circulation_rules?effective=false") != before_rules:
            sys.exit("the oracle's rules did not come back as they were: check them by hand")
    json.dump({
        "koha_version": KOHA_VERSION,
        "spec": ["docs/specs/circulacao/conta.md"],
        "recorder": "tools/fines-recorder/record.py",
        "patron": {"category": "B", "library": "MAIN"},
        "preferences": {**PREFERENCES, FINES_MODE: "production"},
        **record,
    }, sys.stdout, indent=1, ensure_ascii=False)
    print()


if __name__ == "__main__":
    main()
