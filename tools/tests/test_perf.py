# SPDX-License-Identifier: GPL-3.0-or-later
"""The performance measures of the switch rehearsal (docs/specs/migracao/performance.md)."""
import importlib.util
import io
from contextlib import redirect_stdout
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("perf", ROOT / "tools/koha-parity/perf.py")
perf = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(perf)


class PercentileTests(unittest.TestCase):
    def test_nearest_rank(self):
        values = list(range(1, 101))
        self.assertEqual(perf.percentile(values, 0.5), 50)
        self.assertEqual(perf.percentile(values, 0.95), 95)
        self.assertEqual(perf.percentile([7], 0.95), 7)
        self.assertEqual(perf.percentile([], 0.95), 0.0)

    def test_summary_in_milliseconds(self):
        self.assertEqual(perf.summary("x", [0.010, 0.020]),
                         "x: 2 answers, median 10.0 ms, p95 20.0 ms, slowest 20.0 ms")
        self.assertEqual(perf.summary("x", []), "x: no answers")


class RunTests(unittest.TestCase):
    def test_a_command_is_timed_and_its_output_kept_out_of_the_report(self):
        with tempfile.TemporaryDirectory() as folder:
            log = Path(folder) / "log.txt"
            out = io.StringIO()
            with redirect_stdout(out):
                status = perf.main(["run", "echo", "--log", str(log), "--",
                                    sys.executable, "-c", "print('secret record')"])
            self.assertEqual(status, 0)
            self.assertRegex(out.getvalue(), r"^echo: \d+\.\d s, peak memory \d+ MiB, exit 0\n$")
            self.assertEqual(log.read_text().strip(), "secret record")

    def test_the_exit_status_is_kept(self):
        with redirect_stdout(io.StringIO()):
            status = perf.main(["run", "fail", "--", sys.executable, "-c", "raise SystemExit(3)"])
        self.assertEqual(status, 3)


class HttpTests(unittest.TestCase):
    def test_searches_are_repeated_and_records_come_from_the_api(self):
        asked = []

        def get(url):
            asked.append(url)
            if "/api/v1/search" in url:
                return 0.01, json.dumps({"results": [{"instance": 5}, {"instance": 6}]}).encode()
            return 0.02, b"<html>"

        with tempfile.TemporaryDirectory() as folder:
            queries = Path(folder) / "q.json"
            queries.write_text(json.dumps(["dom casmurro"]), encoding="utf-8")
            out = io.StringIO()
            with mock.patch.object(perf, "timed_get", get), redirect_stdout(out):
                perf.main(["http", "http://elzevir/", str(queries), "--repeat", "3"])
        lines = out.getvalue().splitlines()
        self.assertEqual(lines[0], "OPAC search: 3 answers, median 20.0 ms, p95 20.0 ms, slowest 20.0 ms")
        self.assertTrue(lines[1].startswith("API search: 3 answers"))
        self.assertTrue(lines[2].startswith("OPAC record, first opening: 2 answers"))
        self.assertTrue(lines[3].startswith("OPAC record: 2 answers"))
        self.assertIn("http://elzevir/search?q=dom%20casmurro", asked)
        self.assertIn("http://elzevir/record/6", asked)


if __name__ == "__main__":
    unittest.main()
