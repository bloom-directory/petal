#!/usr/bin/env python3
"""Check this experiment's recorded identities/outcomes, not Wasm semantics."""

import csv
import hashlib
import json
import sys
from pathlib import Path

from verdicts import kwasm


ROOT = Path(__file__).resolve().parents[1]
EV = ROOT / "evidence"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def load(path):
    return json.loads(path.read_text())


def table(path):
    with path.open() as stream:
        return list(csv.DictReader(stream, delimiter="\t"))


def check():
    reports = {}
    harnesses = {}
    for variant in ["correct", "faulty"]:
        d = load(EV / variant / "extraction.json")
        reports[variant] = d
        f = d["target_function"]
        h = load(EV / variant / "sunbeam-harness.json")
        harnesses[variant] = h
        reviewed_body = {
            "correct": "c9c2e6618c6f89996bf63ba97fbd617af3e66eb91c63f10401e42ca13d4beba5",
            "faulty": "f74678ec82cd2a52dfa2cc6b9e1144e41308db6b230a18e92c9b956adb3ecc70",
        }
        require(f["body_sha256"] == reviewed_body[variant], "changed body needs a fresh finite-return review")
        body = bytes.fromhex(f["body_hex"])
        require(hashlib.sha256(body).hexdigest() == f["body_sha256"], "body hash")
        require(len(body) == f["body_bytes"], "body length")
        require(f["params"] == ["i32", "i64"] and f["results"] == [], "target ABI")
        require(f["export"] == "bloom_verify_split_even", "target export")
        require(h["target_body_identical"], "harness changed target body")
        require(h["target_body_sha256"] == f["body_sha256"], "harness body identity")
        require(h["shipped_module_sha256"] == d["target_module"]["sha256"], "module identity")
        # Guard the finite, call-free instruction sequence used in the manual
        # normal-return argument. This guard is not another verification backend.
        allowed = {"LocalGet", "LocalTee", "I64Const", "I64ShrU", "I64Sub", "I64Store", "End"}
        require(set(f["opcode_counts"]) <= allowed, "normal-return argument needs fresh review")
        require(f["opcode_counts"].get("I64Store") == 2, "expected two stores")
        r = load(EV / variant / "concrete-run.json")
        require(r["module_sha256"] == d["target_module"]["sha256"], "concrete target identity")
        require(len(r["cases"]) == 18 and r["out_of_bounds_result_slot_traps"], "concrete coverage")
        for case in r["cases"]:
            expected = variant == "correct" or int(case["total"]) % 2 == 0
            require(case["pass"] == expected, "unexpected concrete outcome")
            require(case["conservation"] == expected, "negative control not analyzed")
        require(r["all_pass"] == (variant == "correct"), "concrete aggregate")

    require(reports["correct"]["target_module"]["sha256"] != reports["faulty"]["target_module"]["sha256"], "faulty identity")
    rows = table(EV / "sunbeam/summary.tsv")
    rules = {"rule_split_1_conservation", "rule_split_2_ordered", "rule_split_3_gap",
             "rule_split_4_reaches_return", "rule_split_5_frame", "rule_split_r1_zero",
             "rule_split_r2_odd", "rule_split_r3_max", "probe_oob_result_slot_returns"}
    keys = {(v, r, m, s) for v in reports for r in rules
            for m in ["default", "trapAsAssert"] for s in ["z3", "cvc5"]}
    require(len(rows) == len(keys), "incomplete Sunbeam matrix")
    seen = set()
    for row in rows:
        key = tuple(row[k] for k in ["variant", "rule", "trap_mode", "solver"])
        require(key in keys and key not in seen, "unexpected/duplicate Sunbeam row")
        seen.add(key)
        failed = row["variant"] == "faulty" and row["rule"] in {
            "rule_split_1_conservation", "rule_split_r2_odd", "rule_split_r3_max"}
        require(row["verdict"] == ("FAIL" if failed else "SUCCESS"), "inconclusive/unexpected Sunbeam verdict")
        require(int(row["exit"]) == (100 if failed else 0), "inconsistent Sunbeam exit")
        require(row["harness_sha256"] == harnesses[row["variant"]]["harness_sha256"], "Sunbeam input identity")

    rows = table(EV / "kwasm/summary.tsv")
    keys = {(v, c) for v in reports for c in ["returns", "props", "props-lemma", "frame-below", "frame-above", "exact-lemma"]}
    require(len(rows) == len(keys), "incomplete KWasm matrix")
    seen = set()
    for row in rows:
        variant, claim = row["variant"], row["claim"]
        key = (variant, claim)
        require(key in keys and key not in seen, "unexpected/duplicate KWasm row")
        seen.add(key)
        path = EV / "kwasm" / f"{variant}-{claim}-prove.log"
        classified = kwasm(path.read_text(), claim, int(row["exit"]))
        require(row["verdict"] == classified, "KWasm verdict overstates raw outcome/unchecked lemma")
        expected = ("VERIFIED_IN_TRUSTED_MODEL" if claim.startswith("frame-") or claim == "returns" else
                    "CONDITIONAL_UNPROVED_LEMMA" if variant == "correct" and claim.endswith("-lemma") else
                    "NOT_ESTABLISHED")
        require(classified == expected, "unexpected KWasm outcome; review required")
        spec = (EV / "kwasm" / f"{variant}-{claim}-spec.k").read_text()
        d = reports[variant]
        for sha in [d["archive"]["sha256"], d["component"]["sha256"],
                    d["target_module"]["sha256"], d["target_function"]["body_sha256"]]:
            require(sha in spec, "stale KWasm claim identity")
    print("Recorded artifact identities and expected outcomes agree; no complete proof is claimed.")


if __name__ == "__main__":
    try:
        check()
    except (ValueError, KeyError, OSError) as error:
        sys.exit(f"evidence check failed: {error}")
