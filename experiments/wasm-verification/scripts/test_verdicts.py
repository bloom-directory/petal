"""Regression checks for false-success and unchecked-assumption handling."""

import unittest

from verdicts import kwasm, sunbeam


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

    def test_kwasm_unproved_rewrite_is_never_proof_success(self):
        for claim in ["props-lemma", "exact-lemma"]:
            self.assertEqual(
                kwasm("#Top\n", claim, 0), "CONDITIONAL_UNPROVED_LEMMA"
            )
        self.assertEqual(
            kwasm("#Top\n", "frame-below", 0), "VERIFIED_IN_TRUSTED_MODEL"
        )

    def test_kwasm_errors_timeouts_and_residuals_are_not_established(self):
        for output, code in [
            ("#Top\n", 1), ("#Top\n", 124), ("residual", 1),
            ("unsupported opcode", 1), ("", 0), ("#Top\nerror", 0),
        ]:
            self.assertEqual(kwasm(output, "props", code), "NOT_ESTABLISHED")


if __name__ == "__main__":
    unittest.main()
