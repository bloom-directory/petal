"""Fail-closed classification of backend output; never a proof checker."""

import json
import sys
from pathlib import Path


def sunbeam(report, rule, exit_code):
    try:
        value = json.loads(report)
        # Only the requested rule may supply the verdict.
        status = value["rules"][rule]
    except (ValueError, KeyError, TypeError):
        return "INCONCLUSIVE"
    if status == "SUCCESS" and exit_code == 0:
        return "SUCCESS"
    if status == "FAIL" and exit_code == 100:
        return "FAIL"
    return "INCONCLUSIVE"


def kwasm(log, claim, exit_code):
    if exit_code == 0 and log.strip() == "#Top":
        # These claims import a custom, unproved simplification rule.
        if claim.endswith("-lemma"):
            return "CONDITIONAL_UNPROVED_LEMMA"
        return "VERIFIED_IN_TRUSTED_MODEL"
    # Exit 1 includes residual obligations and frontend errors. It does not
    # establish falsity or supply a counterexample.
    return "NOT_ESTABLISHED"


if __name__ == "__main__":
    backend, path, name, code = sys.argv[1:]
    try:
        output = Path(path).read_text()
    except OSError:
        output = ""
    classify = {"sunbeam": sunbeam, "kwasm": kwasm}[backend]
    print(classify(output, name, int(code)))
