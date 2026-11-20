#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Records how the Koha oracle finds a hold at checkin and sets the Item aside (task 3.8b).

    python3 -I tools/holds-recorder/set_aside.py > scenarios/circulacao/reservas-devolucao.koha.json

Reads the same variables as tools/circ-rules-recorder/record.py and never prints them.

For two records with an Item on the shelf at MAIN, the recorder places a record hold for
the sample patron, checks the Item in at MAIN (`circ/returns.pl`), notes whether Koha found
the hold, confirms it as the staff do (`cud-affect_reserve`) and reads the hold back: the
first time with an empty `holds_pickup_period` rule, the second with three days. Then it
asks whether lending the Item to the sample patron needs a hold confirmation, lends it
(`POST /api/v1/checkouts`), notes whether the hold was filled, and checks the Item in. Afterwards it cancels
every hold of the sample patron and puts the rules back, checking that they are as they
were. Use it only on the disposable test instance, and only with its sample patron.
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
CONTEXT = ("MAIN", "*", "*")
BRANCH = "MAIN"
# The preferences the replay depends on; the recording fails if one is not Koha's default.
PREFERENCES = {
    "ReservesControlBranch": "PatronLibrary",
    "ReservesNeedReturns": "1",
    "ReservesMaxPickUpDelay": "7",
    "ExcludeHolidaysFromMaxPickUpDelay": "0",
    "HoldsAutoFill": "0",
    "ConfirmFutureHolds": "0",
    "LocalHoldsPriority": "None",
    "TrapHoldsOnOrder": "1",
    "AllowHoldPolicyOverride": "0",
    "UseBranchTransferLimits": "0",
}
RULES = {"reservesallowed", "holds_per_record", "holdallowed", "hold_fulfillment_policy",
         "holds_pickup_period", "returnbranch", "fill_other_biblio_holds_policy",
         "maxissueqty", "maxonsiteissueqty", "patron_maxissueqty", "patron_maxonsiteissueqty",
         "issuelength", "lengthunit", "daysmode", "hardduedate", "hardduedatecompare"}
# The hold confirmations of `CanBookBeIssued`.
HELD = {"RESERVE_WAITING", "RESERVED", "TRANSFERRED"}
TOUCHED = {"reservesallowed", "holds_pickup_period"}


class Recorder:
    def __init__(self, oracle):
        self.oracle = oracle
        self.json_headers = {"Accept": "application/json", "Content-Type": "application/json", "CSRF-TOKEN": oracle.csrf}

    def preference(self, name):
        return self.oracle.get("/cgi-bin/koha/svc/config/systempreferences?" + urllib.parse.urlencode({"pref": name}))["value"]

    def rules(self, item_types):
        rules = []
        for raw in self.oracle.get("/api/v1/circulation_rules?effective=false"):
            if raw["context"]["item_type_id"] not in {"*", *item_types}:
                continue
            kept = {name: value for name, value in present(raw).items() if name in RULES}
            if kept:
                rules.append({"context": raw["context"], "rules": kept})
        return rules

    def holds(self):
        return self.oracle.get("/api/v1/holds?" + urllib.parse.urlencode({"patron_id": PATRON, "_per_page": -1}))

    def shelved(self, count):
        """Items on the shelf at MAIN, each of a record with no holds, one per record."""
        query = {"home_library_id": BRANCH, "holding_library_id": BRANCH, "withdrawn": 0, "lost_status": 0,
                 "not_for_loan_status": 0, "checked_out_date": None}
        items = self.oracle.get("/api/v1/items?" + urllib.parse.urlencode({"q": json.dumps(query), "_per_page": 200}))
        chosen, records = [], set()
        for item in sorted(items, key=lambda found: found["item_id"]):
            record = item["biblio_id"]
            if record in records or self.oracle.get("/api/v1/holds?" + urllib.parse.urlencode(
                    {"q": json.dumps({"biblio_id": record}), "_per_page": 1})):
                continue
            records.add(record)
            chosen.append(item)
            if len(chosen) == count:
                return chosen
        sys.exit("not enough Items on the shelf at MAIN")

    def place(self, item):
        body = {"patron_id": PATRON, "biblio_id": item["biblio_id"], "pickup_library_id": BRANCH}
        placed = json.loads(self.oracle.open("/api/v1/holds", data=json.dumps(body).encode(),
                                             method="POST", headers=self.json_headers))
        return placed["hold_id"]

    def check_in(self, item):
        """Checks the Item in and returns the hold Koha offers to set it aside for."""
        page = self.oracle.open("/cgi-bin/koha/circ/returns.pl")
        token = re.search(r'name="csrf_token" value="([^"]+)"', page).group(1)
        form = urllib.parse.urlencode({"op": "cud-checkin", "csrf_token": token,
                                       "barcode": item["external_id"]}).encode()
        page = self.oracle.open("/cgi-bin/koha/circ/returns.pl", data=form)
        found = re.search(r'name="reserve_id" value="(\d+)"', page)
        return int(found.group(1)) if found else None

    def affect(self, item, hold):
        page = self.oracle.open("/cgi-bin/koha/circ/returns.pl")
        token = re.search(r'name="csrf_token" value="([^"]+)"', page).group(1)
        form = urllib.parse.urlencode({
            "op": "cud-affect_reserve", "csrf_token": token, "itemnumber": item["item_id"],
            "borrowernumber": PATRON, "reserve_id": hold, "diffBranch": BRANCH,
        }).encode()
        self.oracle.open("/cgi-bin/koha/circ/returns.pl", data=form)

    def held_confirm(self, item):
        query = urllib.parse.urlencode({"patron_id": PATRON, "item_id": item["item_id"]})
        answer = self.oracle.get(f"/api/v1/checkouts/availability?{query}")
        return bool(HELD & set(answer.get("confirms") or {}))

    def lend(self, item):
        body = json.dumps({"item_id": item["item_id"], "patron_id": PATRON}).encode()
        headers = {**self.json_headers}
        self.oracle.open("/api/v1/checkouts", data=body, method="POST", headers=headers)

    def loans(self):
        return self.oracle.get("/api/v1/checkouts?" + urllib.parse.urlencode({"patron_id": PATRON}))

    def cancel(self, hold):
        self.oracle.open(f"/api/v1/holds/{hold}", method="DELETE", headers=self.json_headers)


def describe(item):
    return {
        "barcode": item["external_id"],
        "item_type": item["item_type_id"],
        "home_library": item["home_library_id"],
        "holding_library": item["holding_library_id"],
    }


def steps(recorder):
    items = recorder.shelved(2)
    record = {"items": [describe(item) for item in items], "steps": []}
    # An empty `holds_pickup_period` falls back to `ReservesMaxPickUpDelay`.
    for item, period in zip(items, ["", "3"]):
        recorder.oracle.put_rules(CONTEXT, {"reservesallowed": "3", "holds_pickup_period": period})
        hold = recorder.place(item)
        found = recorder.check_in(item)
        at = datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")
        if found == hold:
            recorder.affect(item, hold)
        after = next(found_hold for found_hold in recorder.holds() if found_hold["hold_id"] == hold)
        # The holder borrows the Item: Koha asks nothing about the hold and fills it.
        held_confirm = recorder.held_confirm(item)
        recorder.lend(item)
        filled = all(found_hold["hold_id"] != hold for found_hold in recorder.holds())
        recorder.check_in(item)
        record["steps"].append({
            "rules": recorder.rules({item["item_type_id"]}),
            "held_confirm": held_confirm,
            "filled": filled,
            "at": at,
            "found": found == hold,
            "status": after["status"],
            "priority": after["priority"],
            "item_set": after["item_id"] == item["item_id"],
            "waiting_date": after["waiting_date"],
            "expiration_date": after["expiration_date"],
        })
    return record


def main():
    oracle = Oracle()
    oracle.log_in()
    recorder = Recorder(oracle)
    for name, value in PREFERENCES.items():
        if recorder.preference(name) != value:
            sys.exit(f"{name} is not {value!r} on the oracle")
    before_rules = oracle.get("/api/v1/circulation_rules?effective=false")
    library, category, item_type = CONTEXT
    before = next((present(raw) for raw in before_rules
                   if raw["context"] == {"library_id": library, "patron_category_id": category, "item_type_id": item_type}), {})
    if recorder.holds():
        sys.exit("the sample patron already has holds: cancel them first")
    if oracle.get(f"/api/v1/patrons/{PATRON}")["restricted"]:
        sys.exit("the sample patron is restricted: lift it first")
    if recorder.loans():
        sys.exit("the sample patron already has loans: return them first")
    try:
        record = steps(recorder)
    finally:
        for hold in recorder.holds():
            recorder.cancel(hold["hold_id"])
        oracle.put_rules(CONTEXT, {name: before.get(name) for name in TOUCHED})
        for loan in recorder.loans():
            recorder.check_in(oracle.get(f"/api/v1/items/{loan['item_id']}"))
        if recorder.holds():
            sys.exit("the sample patron still has holds: cancel them by hand")
        if recorder.loans():
            sys.exit("the sample patron still has loans: return them by hand")
        if oracle.get("/api/v1/circulation_rules?effective=false") != before_rules:
            sys.exit("the oracle's rules did not come back as they were: check them by hand")
    json.dump({
        "koha_version": KOHA_VERSION,
        "spec": ["docs/specs/circulacao/reservas.md"],
        "recorder": "tools/holds-recorder/set_aside.py",
        "patron": {"category": "B", "library": BRANCH},
        "pickup": BRANCH,
        "preferences": PREFERENCES,
        **record,
    }, sys.stdout, indent=1, ensure_ascii=False)
    print()


if __name__ == "__main__":
    main()
