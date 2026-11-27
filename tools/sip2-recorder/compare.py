#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Compares two recordings of the same SIP2 conversation, one against Koha's server and
one against the Elzevir's on the same copy of the library (docs/specs/migracao/desk-day.md,
part b).

    python3 -I tools/sip2-recorder/compare.py koha.json elzevir.json \\
        --unit 'Main Library=MAIN' --category 'Board=B'

Both files come from `record.py`, which masks the patron's personal fields. The time of
day and the checksum are left out of the comparison, and the known deviations are applied
to Koha's side: the greeting and the `BX` (DEV-0003, DEV-0004), and the unit and category
written by name on the Elzevir's side (DEV-0004, with `--unit` and `--category`).

Prints, for each exchange, whether it is the same, then each difference with both
responses; exits 1 when any exchange differs.
"""

import argparse
import json
import re
import sys

# `YYYYMMDD    HHMMSS`: a moment. A due time of `235900` stays, every other time is masked.
MOMENT = re.compile(r"(\d{8})    (\d{6})")
# Fields that carry a unit or a category.
UNIT_FIELDS = ("AO", "AQ", "AP", "BG", "CT")


def comparable(response):
    """The response without its times of day, which differ by the second, and without the
    checksum, which covers them."""
    text = re.sub(r"(AZ)[0-9A-F]{4}$", r"\1", response)
    return MOMENT.sub(lambda m: m.group(0) if m.group(2) == "235900" else f"{m.group(1)}    NOW___", text)


def as_koha_would(response, units, categories):
    """The Elzevir's response with the unit and category names written as Koha's codes
    (DEV-0004)."""
    for names, fields in ((units, UNIT_FIELDS), (categories, ("PC",))):
        for name, code in names.items():
            for field in fields:
                # The first variable field follows the fixed part with no `|` before it.
                # The terminal may get the name in Latin-1, which `record.py` reads as
                # UTF-8 with replacement characters.
                for written in {name, as_recorded_latin_1(name)}:
                    response = re.sub(rf"(^|[|0-9]){field}{re.escape(written)}(?=\|)",
                                      lambda m, f=field, c=code: f"{m.group(1)}{f}{c}", response)
    return response


def as_recorded_latin_1(text):
    """`text` sent in Latin-1 and read back by `record.py` as UTF-8."""
    return text.encode("latin-1", "replace").decode("utf-8", "replace")


def with_deviations(response):
    """Koha's response as the Elzevir answers it by an approved deviation."""
    return (response.replace("Greetings from Koha.", "Greetings from Elzevir.")
            .replace("|BXYYYYYYYYYYYNYYYY|", "|BXYYYNYYYYYNYNNNYN|"))


def pairs(text):
    """`NAME=CODE` arguments as a dict."""
    found = {}
    for item in text or []:
        name, separator, code = item.rpartition("=")
        if not separator or not name or not code:
            raise SystemExit(f"expected NAME=CODE, got {item!r}")
        found[name] = code
    return found


def compare(koha, elzevir, units, categories):
    """The exchanges in order, each with Koha's and the Elzevir's comparable responses."""
    theirs = {exchange["name"]: exchange for exchange in koha["exchanges"]}
    ours = {exchange["name"]: exchange for exchange in elzevir["exchanges"]}
    rows = []
    for name in theirs:
        if name not in ours:
            rows.append((name, comparable(with_deviations(theirs[name]["response"])), None))
            continue
        rows.append((
            name,
            comparable(with_deviations(theirs[name]["response"])),
            comparable(as_koha_would(ours[name]["response"], units, categories)),
        ))
    return rows


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("koha")
    parser.add_argument("elzevir")
    parser.add_argument("--unit", action="append", help="the Elzevir's unit name=Koha's branchcode")
    parser.add_argument("--category", action="append", help="the Elzevir's category name=Koha's categorycode")
    args = parser.parse_args(argv)
    with open(args.koha, encoding="utf-8") as file:
        koha = json.load(file)
    with open(args.elzevir, encoding="utf-8") as file:
        elzevir = json.load(file)
    rows = compare(koha, elzevir, pairs(args.unit), pairs(args.category))
    different = [(name, theirs, ours) for name, theirs, ours in rows if theirs != ours]
    print(f"{len(rows)} exchanges, {len(rows) - len(different)} the same, {len(different)} different")
    for name, theirs, ours in different:
        print(f"  DIFFERENT {name}\n    Koha:    {theirs}\n    Elzevir: {ours if ours is not None else '(not asked)'}")
    return 1 if different else 0


if __name__ == "__main__":
    sys.exit(main())
