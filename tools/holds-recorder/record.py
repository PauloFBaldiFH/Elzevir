#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Records how the Koha oracle accepts and refuses holds (task 3.8).

    python3 -I tools/holds-recorder/record.py > scenarios/circulacao/reservas.koha.json

Reads the same variables as tools/circ-rules-recorder/record.py and never prints them.

The recorder changes the hold rules of the MAIN branch for the recording and asks Koha's
REST API to place holds for the sample patron on three records, one rule at a time:
record holds, Item holds, a second hold on the same record and Item, the limit per day
and the total limit; it cancels one hold and places another. Afterwards it cancels every
hold of the sample patron and puts the rules back, checking that they are as they were.
Use it only on the disposable test instance, and only with its sample patron.
"""

import json
import os
import sys
import urllib.error
import urllib.parse

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "circ-rules-recorder"))
from record import KOHA_VERSION, Oracle, present  # noqa: E402

PATRON = 5034
CONTEXT = ("MAIN", "*", "*")
PICKUP = "MAIN"
# The preferences the replay depends on; the recording fails if one is not Koha's default.
PREFERENCES = {
    "ReservesControlBranch": "PatronLibrary",
    "AllowHoldsOnDamagedItems": "1",
    "AllowHoldsOnPatronsPossessions": "1",
    "ReservesNeedReturns": "1",
    "AllowHoldPolicyOverride": "0",
    "IndependentBranches": "0",
    "UseBranchTransferLimits": "0",
    "maxoutstanding": "5",
    "maxreserves": "50",
}
RULES = {"reservesallowed", "holds_per_record", "holds_per_day", "max_holds", "holdallowed",
         "hold_fulfillment_policy"}


class Recorder:
    def __init__(self, oracle):
        self.oracle = oracle
        self.json_headers = {"Accept": "application/json", "Content-Type": "application/json", "CSRF-TOKEN": oracle.csrf}

    def preference(self, name):
        return self.oracle.get("/cgi-bin/koha/svc/config/systempreferences?" + urllib.parse.urlencode({"pref": name}))["value"]

    def rules(self):
        rules = []
        for raw in self.oracle.get("/api/v1/circulation_rules?effective=false"):
            kept = {name: value for name, value in present(raw).items() if name in RULES}
            if kept:
                rules.append({"context": raw["context"], "rules": kept})
        return rules

    def holds(self):
        return self.oracle.get("/api/v1/holds?" + urllib.parse.urlencode({"patron_id": PATRON, "_per_page": -1}))

    def records(self):
        """Three records with Items at MAIN, the first with at least two."""
        query = {"home_library_id": "MAIN"}
        items = self.oracle.get("/api/v1/items?" + urllib.parse.urlencode({"q": json.dumps(query), "_per_page": 200}))
        by_record = {}
        for item in items:
            by_record.setdefault(item["biblio_id"], []).append(item)
        first = next((record for record, found in sorted(by_record.items()) if len(found) >= 2), None)
        others = [record for record in sorted(by_record) if record != first][:2]
        if first is None or len(others) < 2:
            sys.exit("not enough records with items at MAIN")
        return [first, *others]

    def items_of(self, record):
        found = self.oracle.get(f"/api/v1/biblios/{record}/items?_per_page=-1")
        return sorted(found, key=lambda item: item["item_id"])

    def place(self, record, item=None):
        body = {"patron_id": PATRON, "biblio_id": record, "pickup_library_id": PICKUP}
        if item is not None:
            body["item_id"] = item["item_id"]
        try:
            placed = json.loads(self.oracle.open("/api/v1/holds", data=json.dumps(body).encode(),
                                                 method="POST", headers=self.json_headers))
            return {"placed": placed["hold_id"], "priority": placed["priority"]}
        except urllib.error.HTTPError as error:
            answer = json.loads(error.read().decode() or "{}")
            return {"status": error.code, "error": answer.get("error")}

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
    records = recorder.records()
    items = {record: recorder.items_of(record) for record in records}
    record = {"records": [[describe(item) for item in items[found]] for found in records], "steps": []}

    def step(rules, record_index, item_index=None):
        if rules is not None:
            recorder.oracle.put_rules(CONTEXT, rules)
        found = records[record_index]
        item = None if item_index is None else items[found][item_index]
        answer = recorder.place(found, item)
        record["steps"].append({
            "rules": recorder.rules(),
            "record": record_index,
            "item": item_index,
            "answer": {key: value for key, value in answer.items() if key != "placed"},
        })
        return answer.get("placed")

    step({"reservesallowed": "0"}, 0)
    on_first = step({"reservesallowed": "3", "holds_per_record": "1"}, 0)
    step(None, 0, 1)
    step(None, 1, 0)
    step(None, 1, 0)
    step({"holds_per_day": "2"}, 2)
    step({"holds_per_day": "", "reservesallowed": "2"}, 2)
    recorder.cancel(on_first)
    record["steps"].append({"cancelled": 0})
    step(None, 2)
    record["queue"] = sorted([records.index(hold["biblio_id"]), hold["priority"]] for hold in recorder.holds())
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
    if oracle.get(f"/api/v1/patrons/{PATRON}/account")["balance"] > 5:
        sys.exit("the sample patron owes too much to place holds: settle it first")
    touched = {"reservesallowed", "holds_per_record", "holds_per_day"}
    try:
        record = steps(recorder)
    finally:
        for hold in recorder.holds():
            recorder.cancel(hold["hold_id"])
        oracle.put_rules(CONTEXT, {name: before.get(name) for name in touched})
        if recorder.holds():
            sys.exit("the sample patron still has holds: cancel them by hand")
        if oracle.get("/api/v1/circulation_rules?effective=false") != before_rules:
            sys.exit("the oracle's rules did not come back as they were: check them by hand")
    json.dump({
        "koha_version": KOHA_VERSION,
        "spec": ["docs/specs/circulacao/reservas.md"],
        "recorder": "tools/holds-recorder/record.py",
        "patron": {"category": "B", "library": "MAIN"},
        "pickup": PICKUP,
        "preferences": PREFERENCES,
        **record,
    }, sys.stdout, indent=1, ensure_ascii=False)
    print()


if __name__ == "__main__":
    main()
