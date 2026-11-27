#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Asks Koha and the Elzevir the same searches on the same copy of the library, through
the OPAC, SRU and Z39.50, and compares the records each one finds by Koha's
`biblionumber` (docs/specs/migracao/desk-day.md, part b, items 15 to 19).

Run it on the oracle VM, next to both systems:

    python3 -I tools/koha-parity/search_compare.py QUERIES.json \\
        --koha-opac http://127.0.0.1 --koha-sru http://127.0.0.1:2100/biblios \\
        --koha-z3950 tcp:127.0.0.1:2100/biblios \\
        --elzevir-api http://127.0.0.1:7391 --elzevir-sru http://127.0.0.1:7391/sru \\
        --elzevir-z3950 tcp:127.0.0.1:2200/biblios

QUERIES.json is a list of `{"kind": "opac" | "cql" | "pqf", "query": "..."}`. Records are
named by `biblionumber` only: Koha's RSS gives it, and both systems write it in `999$c`.
The Elzevir's OPAC set is read through its SRU as one term with a final `*`, which is the
OPAC box with each word completed (sru.md, item 10a); the report checks that both totals
agree. Z39.50 goes
through `zoomsh` (YAZ), as a library's client would.

Prints, for each search, both totals, how many records only one side found, with up to
ten of their numbers, and, for records both found through SRU and Z39.50, those whose
title (`245$a`) or number of Items (`952`) differ. Exits 1 when any search differs.
"""

import argparse
import json
import re
import subprocess
import sys
import urllib.parse
import urllib.request

PAGE = 100
SHOWN = 10
BIBLIONUMBER = re.compile(r"opac-detail\.pl\?biblionumber=(\d+)")
TOTAL_RSS = re.compile(r"<opensearch:totalResults>(\d+)<")
TOTAL_SRU = re.compile(r"<(?:\w+:)?numberOfRecords>(\d+)<")
TOTAL_ZOOM = re.compile(r": (\d+) hits")
RECORD = re.compile(r"<record\b.*?</record>", re.S)
FIELD_999 = re.compile(r'<datafield tag="999"[^>]*>(.*?)</datafield>', re.S)
FIELD_245 = re.compile(r'<datafield tag="245"[^>]*>(.*?)</datafield>', re.S)
FIELD_952 = re.compile(r'<datafield tag="952"')


def subfield(field, code):
    """The first `$code` of a MARCXML field, unescaped and trimmed."""
    found = re.search(rf'<subfield code="{code}">(.*?)</subfield>', field, re.S)
    if not found:
        return None
    text = found.group(1)
    for entity, character in (("&lt;", "<"), ("&gt;", ">"), ("&quot;", '"'), ("&apos;", "'"), ("&amp;", "&")):
        text = text.replace(entity, character)
    return text.strip()


def records(xml):
    """`{biblionumber: (title, Items)}` of the MARCXML records in `xml`."""
    found = {}
    for record in RECORD.findall(xml):
        field = FIELD_999.search(record)
        number = field and subfield(field.group(1), "c")
        if not number:
            continue
        title = FIELD_245.search(record)
        # Koha and the Elzevir end the title with different ISBD marks; compare the words.
        words = title and subfield(title.group(1), "a")
        words = re.sub(r"[\s/:;,.=]+$", "", words or "")
        found[number] = (words, len(FIELD_952.findall(record)))
    return found


def get(url):
    with urllib.request.urlopen(url, timeout=120) as response:
        return response.read().decode("utf-8", "replace")


def koha_opac(base, query, limit):
    """The total and the biblionumbers of Koha's OPAC search, by its RSS feed."""
    numbers, total, offset = [], None, 0
    while total is None or (offset < total and offset < limit):
        rss = get(f"{base}/cgi-bin/koha/opac-search.pl?format=rss&count={PAGE}&offset={offset}&q="
                  + urllib.parse.quote(query))
        found = TOTAL_RSS.search(rss)
        total = int(found.group(1)) if found else 0
        page = BIBLIONUMBER.findall(rss)
        if not page:
            break
        numbers += page
        offset += PAGE
    return total, {number: None for number in numbers}


def sru(base, query, limit):
    """The total and the records of an SRU search, page by page."""
    found, total, start = {}, None, 1
    while total is None or (start <= total and start <= limit):
        xml = get(f"{base}?version=1.1&operation=searchRetrieve&recordSchema=marcxml"
                  f"&startRecord={start}&maximumRecords={PAGE}&query=" + urllib.parse.quote(query))
        match = TOTAL_SRU.search(xml)
        total = int(match.group(1)) if match else 0
        page = records(xml)
        if not page:
            break
        found.update(page)
        # A server may send fewer than asked (its own maximum): go on from the next one.
        start += len(RECORD.findall(xml))
    return total, found


def z3950(target, query, limit):
    """The total and the records of a Z39.50 search through `zoomsh`."""
    first = ["set preferredRecordSyntax xml", "set elementSetName marcxml", f"open {target}",
             f"find {query}"]
    output = subprocess.run(["zoomsh", *first, "quit"], capture_output=True, text=True,
                            timeout=120, check=False).stdout
    match = TOTAL_ZOOM.search(output)
    total = int(match.group(1)) if match else 0
    shows = [f"show {start} {PAGE}" for start in range(0, min(total, limit), PAGE)]
    output = subprocess.run(["zoomsh", *first, *shows, "quit"], capture_output=True, text=True,
                            timeout=600, check=False).stdout
    return total, records(output)


def elzevir_opac(api, sru_base, query, limit):
    """The Elzevir's OPAC search: the total of its API, the records of its SRU."""
    answer = json.loads(get(f"{api}/api/v1/search?q=" + urllib.parse.quote(query)))
    words = " ".join(re.sub(r'["\\*?]', " ", query).split())
    total, found = sru(sru_base, f'"{words}*"', limit)
    return answer["total"], found, total


def compare(theirs, ours, limit):
    """What differs between Koha's `(total, records)` and the Elzevir's."""
    (koha_total, koha), (elzevir_total, elzevir) = theirs, ours
    # Every record read on both sides, or only the totals can be compared.
    complete = len(koha) == koha_total <= limit and len(elzevir) == elzevir_total <= limit
    only_koha = sorted(set(koha) - set(elzevir), key=int) if complete else []
    only_elzevir = sorted(set(elzevir) - set(koha), key=int) if complete else []
    unlike = sorted((number for number in set(koha) & set(elzevir)
                     if koha[number] is not None and elzevir[number] is not None
                     and koha[number] != elzevir[number]), key=int)
    return {
        "koha": koha_total, "elzevir": elzevir_total, "complete": complete,
        "only_koha": only_koha, "only_elzevir": only_elzevir,
        "unlike": [(number, koha[number], elzevir[number]) for number in unlike],
    }


def differs(result):
    return (result["koha"] != result["elzevir"] or result["only_koha"] or result["only_elzevir"]
            or result["unlike"] or result.get("opac_mismatch"))


def report(search, result):
    """The lines of one search in the report."""
    head = f'{search["kind"]} {search["query"]!r}: Koha {result["koha"]}, Elzevir {result["elzevir"]}'
    lines = [("  DIFFERENT " if differs(result) else "  same      ") + head]
    if not result["complete"]:
        lines.append("    not every record was read (more than --max, or a record without 999$c): "
                     "only the totals are compared")
    for side in ("only_koha", "only_elzevir"):
        numbers = result[side]
        if numbers:
            more = f" (+{len(numbers) - SHOWN})" if len(numbers) > SHOWN else ""
            lines.append(f"    {side.replace('_', ' ')}: {len(numbers)}: {', '.join(numbers[:SHOWN])}{more}")
    for number, koha, elzevir in result["unlike"][:SHOWN]:
        lines.append(f"    {number}: Koha {koha}, Elzevir {elzevir}")
    if result.get("opac_mismatch"):
        lines.append(f"    the Elzevir's SRU found {result['opac_mismatch']}, not its OPAC's total")
    return lines


def run(search, args):
    kind, query = search["kind"], search["query"]
    if kind == "opac":
        koha = koha_opac(args.koha_opac, query, args.max)
        total, found, through_sru = elzevir_opac(args.elzevir_api, args.elzevir_sru, query, args.max)
        result = compare(koha, (total, found), args.max)
        if through_sru != total:
            result["opac_mismatch"] = through_sru
        return result
    if kind == "cql":
        return compare(sru(args.koha_sru, query, args.max), sru(args.elzevir_sru, query, args.max), args.max)
    if kind == "pqf":
        return compare(z3950(args.koha_z3950, query, args.max),
                       z3950(args.elzevir_z3950, query, args.max), args.max)
    raise SystemExit(f"unknown kind {kind!r}: opac, cql or pqf")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("queries")
    for name in ("koha-opac", "koha-sru", "koha-z3950", "elzevir-api", "elzevir-sru", "elzevir-z3950"):
        parser.add_argument(f"--{name}", required=True)
    parser.add_argument("--max", type=int, default=5000, help="most records read per search")
    args = parser.parse_args(argv)
    with open(args.queries, encoding="utf-8") as file:
        searches = json.load(file)
    lines, different = [], 0
    for search in searches:
        result = run(search, args)
        different += bool(differs(result))
        lines += report(search, result)
    print(f"{len(searches)} searches, {len(searches) - different} the same, {different} different")
    print("\n".join(lines))
    return 1 if different else 0


if __name__ == "__main__":
    sys.exit(main())
