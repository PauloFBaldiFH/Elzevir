#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Records how the Koha oracle renews a loan and suspends a late return (task 3.5).

    python3 -I tools/checkin-recorder/record.py > scenarios/circulacao/devolucao-renovacao.koha.json

Reads the same variables as tools/circ-rules-recorder/record.py and never prints them.

The recorder adds a few circulation rules and sets `ManualRenewalPeriodBase` to Koha's
default for the recording, lends one Item to the sample patron, asks whether it may be
renewed before and after renewing it, returns it late (`SpecifyReturnDate`), and asks
whether another Item may be lent then. Afterwards it lifts the suspension, puts the rules
and the preference back and checks that the patron and the oracle are as they were. Use
it only on the disposable test instance, and only with its sample patron.
"""

import datetime
import json
import os
import re
import sys
import urllib.parse

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "circ-rules-recorder"))
from record import KOHA_VERSION, Oracle, present  # noqa: E402

PATRON = 5034
CONTEXT = ("MAIN", "*", "DV")
# No fine, so the late return leaves no debt; two days of suspension for each day late; one
# renewal, not before three days from the due date.
TEMPORARY = {"fine": "0", "finedays": "2", "renewalsallowed": "1", "norenewalbefore": "3"}
# The preferences the replay depends on; the recording fails if one is not Koha's default.
PREFERENCES = {
    "CircControl": "ItemHomeLibrary",
    "HomeOrHoldingBranch": "holdingbranch",
    "useDaysMode": "Calendar",
    "NoRenewalBeforePrecision": "exact_time",
    "OverduesBlockRenewing": "allow",
    "RestrictionBlockRenewing": "0",
    "SuspensionsCalendar": "noSuspensionsWhenClosed",
    "finesCalendar": "noFinesWhenClosed",
    "CumulativeRestrictionPeriods": "0",
    "SpecifyReturnDate": "1",
}
RENEWAL_BASE = "ManualRenewalPeriodBase"
# The rules the replay loads.
RULES = {
    "maxissueqty", "maxonsiteissueqty", "patron_maxissueqty", "patron_maxonsiteissueqty",
    "issuelength", "renewalperiod", "lengthunit", "daysmode", "hardduedate", "hardduedatecompare",
    "renewalsallowed", "norenewalbefore", "finedays", "firstremind", "maxsuspensiondays",
    "suspension_chargeperiod", "returnbranch",
}
DAYS_LATE = 4


class Recorder:
    def __init__(self, oracle):
        self.oracle = oracle
        self.json_headers = {"Accept": "application/json", "Content-Type": "application/json", "CSRF-TOKEN": oracle.csrf}

    def preference(self, name):
        return self.oracle.get("/cgi-bin/koha/svc/config/systempreferences?" + urllib.parse.urlencode({"pref": name}))["value"]

    def set_preference(self, name, value):
        form = urllib.parse.urlencode({"value": value, "csrf_token": self.oracle.csrf}).encode()
        self.oracle.open(f"/cgi-bin/koha/svc/config/systempreferences/{name}", data=form,
                         headers={"CSRF-TOKEN": self.oracle.csrf})
        if self.preference(name) != value:
            sys.exit(f"could not set {name}")

    def available(self, count):
        query = {"item_type_id": "DV", "home_library_id": "MAIN", "holding_library_id": "MAIN",
                 "withdrawn": 0, "lost_status": 0, "not_for_loan_status": 0, "damaged_status": 0,
                 "restricted_status": [0, None], "checked_out_date": None}
        found = self.oracle.get("/api/v1/items?" + urllib.parse.urlencode({"q": json.dumps(query), "_per_page": count}))
        if len(found) < count:
            sys.exit("not enough available items")
        return found

    def rules(self):
        rules = []
        for raw in self.oracle.get("/api/v1/circulation_rules?effective=false"):
            kept = {name: value for name, value in present(raw).items() if name in RULES}
            if kept:
                rules.append({"context": raw["context"], "rules": kept})
        return rules

    def loans(self, checked_in):
        query = {"patron_id": PATRON}
        if checked_in:
            query["checked_in"] = "true"
        return self.oracle.get("/api/v1/checkouts?" + urllib.parse.urlencode(query))

    def lend(self, item):
        body = json.dumps({"item_id": item["item_id"], "patron_id": PATRON}).encode()
        return json.loads(self.oracle.open("/api/v1/checkouts", data=body, method="POST", headers=self.json_headers))

    def allows_renewal(self, checkout):
        asked_at = now()
        answer = self.oracle.get(f"/api/v1/checkouts/{checkout['checkout_id']}/allows_renewal")
        return {"at": asked_at, "answer": answer}

    def renew(self, checkout):
        renewed = json.loads(self.oracle.open(f"/api/v1/checkouts/{checkout['checkout_id']}/renewal",
                                              data=b"{}", method="POST", headers=self.json_headers))
        return {"due": renewed["due_date"], "at": loan_of(self.loans(False), checkout)["last_renewed_date"]}

    def give_back(self, item, when):
        page = self.oracle.open("/cgi-bin/koha/circ/returns.pl")
        token = re.search(r'name="csrf_token" value="([^"]+)"', page).group(1)
        form = urllib.parse.urlencode({
            "op": "cud-checkin", "csrf_token": token, "barcode": item["external_id"],
            # `dateformat` is `metric` on the oracle.
            "return_date_override": when.strftime("%d/%m/%Y %H:%M"),
        }).encode()
        self.oracle.open("/cgi-bin/koha/circ/returns.pl", data=form)

    def availability(self, item):
        query = urllib.parse.urlencode({"patron_id": PATRON, "item_id": item["item_id"]})
        asked_at = now()
        answer = self.oracle.get(f"/api/v1/checkouts/availability?{query}")
        answer.pop("confirmation_token", None)
        return {"at": asked_at, "answer": answer}

    def lift_suspensions(self):
        page = self.oracle.open(f"/cgi-bin/koha/members/moremember.pl?borrowernumber={PATRON}")
        for restriction in re.findall(r'name="borrower_debarment_id" value="(\d+)"', page):
            form = urllib.parse.urlencode({"op": "cud-del", "csrf_token": self.oracle.csrf,
                                           "borrowernumber": PATRON, "borrower_debarment_id": restriction}).encode()
            self.oracle.open("/cgi-bin/koha/members/mod_debarment.pl", data=form)

    def calendar(self, branch):
        """The staff calendar screen lists every entry in JavaScript arrays."""
        page = self.oracle.open(f"/cgi-bin/koha/tools/holidays.pl?branch={branch}")

        def keys(array):
            # `holidays["` also matches inside `day_month_holidays["`.
            return re.findall(r'(?<![A-Za-z0-9_])' + array + r'\["([^"]*)"', page)

        def dates(array):
            return sorted("%04d-%02d-%02d" % tuple(int(part) for part in key.split("/")) for key in keys(array))

        return {
            "weekly_closed": sorted(int(day) for day in keys("week_days")),
            "annual": sorted([int(part) for part in key.split("/")] for key in keys("day_month_holidays")),
            "closed": dates("holidays"),
            "open": dates("exception_holidays"),
        }


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")


def loan_of(loans, checkout):
    return next(loan for loan in loans if loan["checkout_id"] == checkout["checkout_id"])


def describe(item):
    return {
        "barcode": item["external_id"],
        "item_type": item["item_type_id"],
        "home_library": item["home_library_id"],
        "holding_library": item["holding_library_id"],
    }


def steps(recorder):
    lent, other = recorder.available(2)
    checkout = recorder.lend(lent)
    loan = loan_of(recorder.loans(False), checkout)
    record = {
        "item": describe(lent),
        "other_item": describe(other),
        "rules": recorder.rules(),
        "calendar": recorder.calendar("MAIN"),
        "checkout": {"at": loan["checkout_date"], "due": loan["due_date"], "library": loan["library_id"]},
        "too_soon": recorder.allows_renewal(checkout),
    }
    recorder.oracle.put_rules(CONTEXT, {"norenewalbefore": ""})
    record["renewal_rules"] = recorder.rules()
    record["renewal"] = recorder.renew(checkout)
    record["too_many"] = recorder.allows_renewal(checkout)
    due = datetime.datetime.fromisoformat(record["renewal"]["due"])
    back = due.replace(hour=10, minute=0, second=0) + datetime.timedelta(days=DAYS_LATE)
    recorder.give_back(lent, back)
    returned = loan_of(recorder.loans(True), checkout)
    record["checkin"] = {"at": returned["checkin_date"], "library": "MAIN"}
    record["suspended"] = recorder.availability(other)
    return record


def main():
    oracle = Oracle()
    oracle.log_in()
    recorder = Recorder(oracle)
    for name, value in PREFERENCES.items():
        if recorder.preference(name) != value:
            sys.exit(f"{name} is not {value} on the oracle")
    before_rules = oracle.get("/api/v1/circulation_rules?effective=false")
    before_base = recorder.preference(RENEWAL_BASE)
    before_balance = oracle.get(f"/api/v1/patrons/{PATRON}/account")["balance"]
    if recorder.loans(False):
        sys.exit("the sample patron already has loans: return them first")
    if oracle.get(f"/api/v1/patrons/{PATRON}")["restricted"]:
        sys.exit("the sample patron is restricted: lift it first")
    try:
        recorder.set_preference(RENEWAL_BASE, "date_due")
        oracle.put_rules(CONTEXT, TEMPORARY)
        record = steps(recorder)
    finally:
        for loan in recorder.loans(False):
            item = oracle.get(f"/api/v1/items/{loan['item_id']}")
            recorder.give_back(item, datetime.datetime.now())
        recorder.lift_suspensions()
        oracle.put_rules(CONTEXT, {name: None for name in TEMPORARY})
        recorder.set_preference(RENEWAL_BASE, before_base)
        if recorder.loans(False):
            sys.exit("the sample patron still has loans: return them by hand")
        if oracle.get(f"/api/v1/patrons/{PATRON}")["restricted"]:
            sys.exit("the sample patron is still restricted: lift it by hand")
        if oracle.get(f"/api/v1/patrons/{PATRON}/account")["balance"] != before_balance:
            sys.exit("the sample patron's balance changed: check the account by hand")
        if oracle.get("/api/v1/circulation_rules?effective=false") != before_rules:
            sys.exit("the oracle's rules did not come back as they were: check them by hand")
    json.dump({
        "koha_version": KOHA_VERSION,
        "spec": ["docs/specs/circulacao/renovacao.md", "docs/specs/circulacao/suspensao.md"],
        "recorder": "tools/checkin-recorder/record.py",
        "patron": {"category": "B", "library": "MAIN"},
        "preferences": {**PREFERENCES, RENEWAL_BASE: "date_due"},
        **record,
    }, sys.stdout, indent=1, ensure_ascii=False)
    print()


if __name__ == "__main__":
    main()
