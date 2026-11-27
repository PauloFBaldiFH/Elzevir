# SPDX-License-Identifier: GPL-3.0-or-later
"""The SIP2 comparison of part b of the desk day (docs/specs/migracao/desk-day.md)."""
import importlib.util
import io
from contextlib import redirect_stdout
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("compare", ROOT / "tools/sip2-recorder/compare.py")
compare = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(compare)


def recording(*exchanges):
    return {"exchanges": [{"name": name, "request": "", "response": response} for name, response in exchanges]}


class CompareTests(unittest.TestCase):
    def run_main(self, koha, elzevir, *options):
        with tempfile.TemporaryDirectory() as folder:
            paths = []
            for name, content in (("koha.json", koha), ("elzevir.json", elzevir)):
                path = Path(folder) / name
                path.write_text(json.dumps(content), encoding="utf-8")
                paths.append(str(path))
            out = io.StringIO()
            with redirect_stdout(out):
                code = compare.main([*paths, *options])
            return code, out.getvalue()

    def test_times_checksums_and_deviations_do_not_count(self):
        koha = recording(
            ("patron_status", "24              00120261127    101502AOMAIN|AAPATRON|AENAME|BLY|AFGreetings from Koha.|AY2AZE1A0"),
            ("checkout", "121NNY20261127    101503AOMAIN|AAPATRON|ABITEM|AH20261211    235900|AY3AZD3C1"),
            ("sc_status", "98YYYNYN10000520261127    1015002.00AOMAIN|BXYYYYYYYYYYYNYYYY|AY1AZF0A1"),
        )
        elzevir = recording(
            ("patron_status", "24              00120261127    101544AOBiblioteca Central|AAPATRON|AENAME|BLY|AFGreetings from Elzevir.|AY2AZ0000"),
            ("checkout", "121NNY20261127    101545AOBiblioteca Central|AAPATRON|ABITEM|AH20261211    235900|AY3AZ1111"),
            ("sc_status", "98YYYNYN10000520261127    1015402.00AOBiblioteca Central|BXYYYNYYYYYNYNNNYN|AY1AZ2222"),
        )
        code, out = self.run_main(koha, elzevir, "--unit", "Biblioteca Central=MAIN")
        self.assertEqual(code, 0, out)
        self.assertEqual(out, "3 exchanges, 3 the same, 0 different\n")

    def test_a_different_due_date_or_missing_exchange_is_reported(self):
        koha = recording(
            ("checkout", "121NNY20261127    101503AOMAIN|AH20261211    235900|AY3AZD3C1"),
            ("checkin", "101YNN20261127    101510AOMAIN|AY4AZD3C2"),
        )
        elzevir = recording(("checkout", "121NNY20261127    101545AOMAIN|AH20261212    235900|AY3AZD3C1"))
        code, out = self.run_main(koha, elzevir)
        self.assertEqual(code, 1)
        self.assertIn("2 exchanges, 0 the same, 2 different", out)
        self.assertIn("AH20261212    235900", out)
        self.assertIn("Elzevir: (not asked)", out)

    def test_a_category_name_sent_in_latin_1_is_still_its_code(self):
        koha = recording(("patron_information", "64              00120261127    101502AOMAIN|PCB|AY5AZD4F4"))
        elzevir = recording(("patron_information", "64              00120261127    101544AOMAIN|PCBibliotec\ufffdrio|AY5AZ0000"))
        code, out = self.run_main(koha, elzevir, "--category", "Bibliotecário=B")
        self.assertEqual(code, 0, out)

    def test_names_are_pairs(self):
        with self.assertRaises(SystemExit):
            compare.pairs(["no-equals"])
        self.assertEqual(compare.pairs(["A=B=C"]), {"A=B": "C"})


if __name__ == "__main__":
    unittest.main()
