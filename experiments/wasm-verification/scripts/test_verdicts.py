"""Regression checks for false-success and incomplete-result handling."""

import unittest

from verdicts import sunbeam


class VerdictTests(unittest.TestCase):
    def test_sunbeam_requires_report_and_consistent_process_status(self):
        report = '{"rules":{"sum":"SUCCESS"}}'
        self.assertEqual(sunbeam(report, "sum", 0), "SUCCESS")
        for code in [1, 100, 124, 137]:
            self.assertEqual(sunbeam(report, "sum", code), "INCONCLUSIVE")
        for output in ["", "unsupported instruction", "{}", "null", "[]"]:
            self.assertEqual(sunbeam(output, "sum", 0), "INCONCLUSIVE")
        self.assertEqual(sunbeam(report, "missing", 0), "INCONCLUSIVE")

    def test_sunbeam_incomplete_and_unknown_are_inconclusive(self):
        for status in ["TIMEOUT", "UNKNOWN", "ERROR", "SKIPPED"]:
            report = '{"rules":{"sum":"' + status + '"}}'
            self.assertEqual(sunbeam(report, "sum", 0), "INCONCLUSIVE")
        report = '{"rules":{"sum":"FAIL"}}'
        self.assertEqual(sunbeam(report, "sum", 100), "FAIL")
        self.assertEqual(sunbeam(report, "sum", 0), "INCONCLUSIVE")

if __name__ == "__main__":
    unittest.main()
