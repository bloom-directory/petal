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


if __name__ == "__main__":
    path, name, code = sys.argv[1:]
    try:
        output = Path(path).read_text()
    except OSError:
        output = ""
    print(sunbeam(output, name, int(code)))
