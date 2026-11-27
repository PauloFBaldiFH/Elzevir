# SPDX-License-Identifier: GPL-3.0-or-later
"""The search comparison of part b of the desk day (docs/specs/migracao/desk-day.md)."""
import importlib.util
from pathlib import Path
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("search_compare", ROOT / "tools/koha-parity/search_compare.py")
search_compare = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(search_compare)


def record(number, title, items=0):
    fields = [f'<datafield tag="245" ind1="1" ind2="0"><subfield code="a">{title}</subfield></datafield>']
    fields += ['<datafield tag="952" ind1=" " ind2=" "><subfield code="p">1</subfield></datafield>'] * items
    fields.append(f'<datafield tag="999" ind1=" " ind2=" "><subfield code="c">{number}</subfield></datafield>')
    return "<record>" + "".join(fields) + "</record>"


def sru_page(total, *records):
    return f"<numberOfRecords>{total}</numberOfRecords>" + "".join(records)


class RecordsTests(unittest.TestCase):
    def test_a_record_is_named_by_999c_with_its_title_words_and_items(self):
        xml = record(7, "Dom Casmurro /", 2) + record(8, "A &amp; B :") + "<record>no 999</record>"
        self.assertEqual(search_compare.records(xml), {"7": ("Dom Casmurro", 2), "8": ("A & B", 0)})


class PagingTests(unittest.TestCase):
    def test_sru_goes_on_from_the_records_it_got(self):
        pages = [sru_page(3, record(1, "a"), record(2, "b")), sru_page(3, record(3, "c"))]
        asked = []

        def get(url):
            asked.append(url)
            return pages.pop(0)

        with mock.patch.object(search_compare, "get", get):
            total, found = search_compare.sru("http://sru", "x", 100)
        self.assertEqual((total, sorted(found)), (3, ["1", "2", "3"]))
        # The server sent two of the hundred asked: the next page starts at the third.
        self.assertIn("startRecord=3", asked[1])

    def test_the_elzevir_opac_reads_one_term_with_every_word_completed(self):
        asked = []

        def get(url):
            asked.append(url)
            if "/api/v1/search" in url:
                return '{"total": 1}'
            return sru_page(1, record(5, "x"))

        with mock.patch.object(search_compare, "get", get):
            total, found, through_sru = search_compare.elzevir_opac("http://api", "http://sru", 'historia "do" bra*sil?', 10)
        self.assertEqual((total, list(found), through_sru), (1, ["5"], 1))
        self.assertIn("query=" + search_compare.urllib.parse.quote('"historia do bra sil*"'), asked[1])


class CompareTests(unittest.TestCase):
    def test_records_on_one_side_and_different_records_are_found(self):
        koha = (3, {"1": ("A", 1), "2": ("B", 1), "3": ("C", 2)})
        elzevir = (3, {"1": ("A", 1), "2": ("B", 0), "4": ("D", 1)})
        result = search_compare.compare(koha, elzevir, 100)
        self.assertTrue(result["complete"])
        self.assertEqual((result["only_koha"], result["only_elzevir"]), (["3"], ["4"]))
        self.assertEqual(result["unlike"], [("2", ("B", 1), ("B", 0))])
        self.assertTrue(search_compare.differs(result))

    def test_only_the_totals_count_when_a_side_was_not_read_whole(self):
        result = search_compare.compare((5, {"1": None}), (5, {"2": ("x", 0)}), 2)
        self.assertFalse(result["complete"])
        self.assertEqual((result["only_koha"], result["only_elzevir"]), ([], []))
        self.assertFalse(search_compare.differs(result))
        self.assertIn("only the totals", "\n".join(search_compare.report({"kind": "opac", "query": "q"}, result)))

    def test_an_opac_total_the_sru_does_not_repeat_is_a_difference(self):
        result = search_compare.compare((1, {"1": None}), (1, {"1": ("a", 0)}), 10)
        self.assertFalse(search_compare.differs(result))
        result["opac_mismatch"] = 2
        self.assertTrue(search_compare.differs(result))


if __name__ == "__main__":
    unittest.main()
