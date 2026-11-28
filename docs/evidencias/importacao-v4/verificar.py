# SPDX-License-Identifier: GPL-3.0-or-later
"""Offline development oracles. Run with Python 3.11+ from any directory."""
import argparse
import csv
import gzip
import hashlib
import json
import re
import subprocess
import zipfile
from collections import Counter
from pathlib import Path
from xml.etree import ElementTree

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
CORPUS = ROOT / "samples-import-tool"
FIXTURES = ROOT / "crates/elzevir-store/tests/data/import-v4"


def verify_manifest():
    with (HERE / "perfis.tsv").open() as sources:
        profiles = {r["perfil"] for r in csv.DictReader(sources, delimiter="\t")}
    with (HERE / "corpus.tsv").open() as manifest:
        for row in csv.DictReader(manifest, delimiter="\t"):
            assert row["perfil"] in profiles
            path = CORPUS / row["arquivo"]
            assert path.stat().st_size == int(row["bytes"])
            with path.open("rb") as src:
                assert hashlib.file_digest(src, "sha256").hexdigest() == row["sha256"], path
    with zipfile.ZipFile(CORPUS / "evergreen-sample.zip") as archive:
        with (HERE / "evergreen.tsv").open() as manifest:
            members = list(csv.DictReader(manifest, delimiter="\t"))
        assert {r["membro"] for r in members} == {m.filename for m in archive.infolist() if not m.is_dir()}
        for row in members:
            with archive.open(row["membro"]) as src:
                assert hashlib.file_digest(src, "sha256").hexdigest() == row["sha256"]


def copy_rows(lines):
    """Recognize only the COPY text headers and terminators in the two fixed fixtures."""
    table = None
    for line in lines:
        line = line.rstrip("\r\n")
        if table is None:
            header = re.fullmatch(r"COPY ([\w.]+)\s*\(([^)]+)\) FROM stdin;", line)
            if header:
                table, columns = header.groups()
                columns = [c.strip() for c in columns.split(",")]
        elif line == r"\.":
            table = None
        else:
            yield table, dict(zip(columns, line.split("\t"), strict=True))
    assert table is None


def verify_source_oracles():
    counts = Counter()
    keys = set()
    with (CORPUS / "BKP_BIBLIOTECA (2).backup").open(encoding="utf-8") as src:
        for table, row in copy_rows(src):
            counts[table] += 1
            if table == "public.acervo":
                keys.add(row["mfn"])
                for field in ("titulo", "registro"):
                    counts[field] += row[field].strip() in ("", r"\N")
            else:
                counts["missing_link"] += row["cod_livro"].strip() in ("", r"\N")
    assert (counts["public.acervo"], len(keys), counts["titulo"], counts["registro"]) == (11561, 11561, 72, 18)
    assert (counts["public.ocorrencias"], counts["missing_link"]) == (5, 3)
    # In this fixed export VALUES starts a block and each physical data line
    # holds one tuple. Verify both delimiters before counting the line.
    expected = {"biblio": 18919, "biblio_metadata": 18919, "items": 30034,
                "borrowers": 1, "auth_header": 1, "issues": 0, "old_issues": 10}
    declarations = set()
    rows = Counter()
    size = 0
    table = None
    with gzip.open(CORPUS / "koha sql sample.gz", "rb") as src:
        for line in src:
            size += len(line)
            if table is not None:
                value = line.rstrip()
                assert value.startswith(b"(") and value.endswith((b"),", b");"))
                rows[table] += 1
                if value.endswith(b";"):
                    table = None
                continue
            ddl = re.fullmatch(rb"CREATE TABLE `(\w+)` \(", line.rstrip())
            if ddl:
                declarations.add(ddl[1].decode("ascii"))
            statement = re.fullmatch(rb"INSERT INTO `(\w+)` VALUES", line.rstrip())
            if statement and statement[1].decode("ascii") in expected:
                table = statement[1].decode("ascii")
    assert table is None
    assert len(declarations) == 297 and expected.keys() <= declarations
    assert size == 149654596 and all(rows[t] == n for t, n in expected.items())


def verify_openbiblio():
    # Read only the numeric tuple prefix. Column order differs across fragments.
    prefix = re.compile(rb"(?:VALUES\s*|^\s*)\(\s*(\d+),\s*(\d+|')", re.I)
    keys = {}
    for table in ("biblio", "biblio_copy", "biblio_field", "biblio_subfield"):
        with (CORPUS / "04-openbiblio" / (table + ".sql")).open("rb") as src:
            keys[table] = [m.groups() for line in src if (m := prefix.search(line))]
    assert [len(rows) for rows in keys.values()] == [22, 93, 604, 1299]
    bib_ids = {row[0] for row in keys["biblio"]}
    assert all(row[0] in bib_ids for table in keys.values() for row in table)
    assert set(keys["biblio_subfield"]) <= set(keys["biblio_field"])


def verify_biblivre_archive(pg_restore):
    # Ask the optional local client for COPY text on stdout, with no target DB.
    archive = CORPUS / "biblivre-lab/biblivre_teste.dump"
    expected = {"biblio_records": 10, "biblio_holdings": 20, "users": 10,
                "authorities_records": 0, "lendings": 0}
    records = set()
    for table, count in expected.items():
        args = [pg_restore, "--data-only", "--file=-", "--schema=single", "--table", table, archive]
        with subprocess.Popen(args, stdout=subprocess.PIPE, text=True, encoding="utf-8") as proc:
            seen = 0
            in_block = False
            found = False
            for line in proc.stdout:
                if line.startswith("COPY single." + table + " "):
                    found = in_block = True
                elif line.rstrip() == r"\.":
                    in_block = False
                elif in_block:
                    seen += 1
                    if table == "biblio_records":
                        records.add(line.split("\t", 1)[0])
                    elif table == "biblio_holdings":
                        assert line.split("\t", 2)[1] in records
            assert proc.wait() == 0 and found and not in_block and seen == count, table
    assert len(records) == 10


def verify_fixture():
    # Independently specified keys and relationships, including two pending edges.
    tables = {}
    with (FIXTURES / "mixed.sql").open() as src:
        for name, row in copy_rows(src):
            tables.setdefault(name, []).append(row)
    assert [(r["id"], r["description"], r["accession"]) for r in tables["units"]] == [
        ("01", "01", "000010"), ("02", "01", "000011"), ("03", "02", "000012")]
    assert [(r["id"], r["card"]) for r in tables["registrations"]] == [("01", "000007"), ("02", "000008")]
    assert [(r["description"], r["heading"], r["role"]) for r in tables["contributions"]] == [
        ("01", "A1", "aut"), ("02", "A9", "ill")]
    assert [(r["registration"], r["unit"], r["returned"]) for r in tables["movements"]] == [
        ("01", "02", r"\N"), ("02", "01", "2001-01-04"), ("99", "03", r"\N")]
    authorities = ElementTree.parse(FIXTURES / "authorities.xml").getroot()
    ids = [r.findtext("{*}controlfield[@tag='001']") for r in authorities]
    assert ids == [r["id"] for r in tables["headings"]] == ["A1", "A2"]
    links = [r.findtext("{*}datafield[@tag='500']/{*}subfield[@code='0']") for r in authorities]
    assert links == ["(example)A2", "(example)A1"]
    return tables


def folio_parts(name):
    path = CORPUS / "02-folio/datasets/cornell" / (name + ".tsv")
    with path.open() as src:
        return [json.loads(line.split("\t")[1]) for line in src]


def experiment(tables):
    # All cases concern candidates for review. Neither method writes entities.
    items = folio_parts("items")
    holdings = folio_parts("recordholdings")
    instances = folio_parts("instances")
    def values(rows, key):
        return [r[key] for r in rows]

    local_numbers = ["01", "02"]
    cases = [
        # name, child values, parent values, same source, independent role evidence, valid complete relationship
        ("FOLIO item → holding", values(items, "holdingsRecordId"), values(holdings, "id"), True, True, True),
        ("FOLIO holding → instance", values(holdings, "instanceId"), values(instances, "id"), True, True, True),
        ("COPY unit → description", values(tables["units"], "description"), values(tables["descriptions"], "id"), True, True, True),
        ("same numbers in staff", values(tables["units"], "description"), values(tables["operators"], "id"), True, False, False),
        ("independent source", local_numbers, local_numbers, False, True, False),
        ("duplicate parent", local_numbers, ["01", "02", "02"], True, True, False),
        ("missing registration", values(tables["movements"], "registration"), values(tables["registrations"], "id"), True, True, False),
        ("relationship with profile disabled", values(holdings, "instanceId"), values(instances, "id"), True, False, True),
    ]
    scores = [Counter(), Counter()]
    for name, children, parents, same_source, evidenced, valid in cases:
        included = bool(children) and set(children) <= set(parents)
        guarded = included and same_source and evidenced and len(parents) == len(set(parents))
        for score, suggestion in zip(scores, (included, guarded), strict=True):
            score[("TP" if valid else "FP") if suggestion else ("FN" if valid else "TN")] += 1
        print(name, int(included), int(guarded), int(valid), sep="\t")
    assert scores == [Counter(TP=4, FP=3, TN=1), Counter(TP=3, FN=1, TN=4)]
    print("inclusion", dict(scores[0]), "guarded", dict(scores[1]))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pg-restore", help="also verify the PGDMP using a local PostgreSQL 17 client")
    args = parser.parse_args()
    verify_manifest()
    verify_openbiblio()
    if args.pg_restore:
        verify_biblivre_archive(args.pg_restore)
    verify_source_oracles()
    experiment(verify_fixture())
