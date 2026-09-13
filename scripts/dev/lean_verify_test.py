#!/usr/bin/env python3
"""Verify a battery of rMath claims in Lean (slow: Mathlib loads per claim).

This is the correctness net for the Lean bridge: it includes claims that must be
*proved* and claims that must be *rejected*, because a verifier that says
"verified" for a false statement is worse than no verifier.

    make lean-verify
"""

import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, ROOT)
from rmathpy import lean  # noqa: E402

# (rMath input, kernel result, expected outcome)
BATTERY = [
    # arithmetic, algebra, special values
    ("1/2 + 1/3", "5/6", "verified"),
    ("2^10", "1024", "verified"),
    ("Abs[-7]", "7", "verified"),
    ("Sqrt[2]^2", "2", "verified"),
    ("Sin[Pi/6]", "1/2", "verified"),
    ("20! == 2432902008176640000", None, "verified"),
    ("Factor[x^2 - 1]", "(-1 + x) (1 + x)", "verified"),
    ("Expand[(x + 1)^3]", "1 + 3 x + 3 x^2 + x^3", "verified"),
    ("x^2 + 2 x + 1 == (x + 1)^2", None, "verified"),
    ("x^2 >= 0", None, "verified"),
    # derivatives: the shapes the HasDerivAt wiring covers
    ("D[x, x]", "1", "verified"),
    ("D[7, x]", "0", "verified"),
    ("D[Cos[x], x]", "-Sin[x]", "verified"),
    ("D[3 x^4, x]", "12 x^3", "verified"),
    ("D[Exp[x^2], x]", "2 x Exp[x^2]", "verified"),
    ("D[x^3 + Sin[x], x]", "Cos[x] + 3 x^2", "verified"),
    ("D[x^2 + 2 x + 1, x]", "2 + 2 x", "verified"),
    ("D[Sin[x]^2, x]", "2 Cos[x] Sin[x]", "verified"),
    ("D[1/x, x]", "-1/x^2", "verified"),
    ("D[Log[x], x]", "1/x", "verified"),
    ("D[Sqrt[x], x]", "1/2 1/Sqrt[x]", "verified"),
    ("D[Sin[x]/x, x]", "Cos[x]/x - 1/x^2 Sin[x]", "verified"),
    ("D[Exp[x] Log[x], x]", "Exp[x] Log[x] + Exp[x]/x", "verified"),
    ("D[Log[x^2 + 1], x]", "2 x/(1 + x^2)", "verified"),
    # definite integrals (intervalIntegral.integral_pow)
    ("Integrate[x^2, {x, 0, 1}]", "1/3", "verified"),
    ("Integrate[x^3, {x, 0, 2}]", "4", "verified"),
    # must NOT be accepted
    ("1/2 + 1/3", "2/3", "unproved"),
    ("2 + 2", "5", "unproved"),
    ("x^2 + 2 x + 1", "0", "unproved"),
    # must be refused with a reason, not guessed at
    ("N[Pi]", "3.14159", "cannot verify"),
    ("2 I", "0 + 2 I", "cannot verify"),
    ("GCD[24, 36]", "12", "cannot verify"),
]


def main():
    if not lean.available():
        print("LEAN-VERIFY: SKIP (Lean is not installed)")
        return 0
    ok = bad = 0
    for lhs, rhs, want in BATTERY:
        r = lean.verify(lhs, rhs, timeout=600)
        got = r.get("outcome")
        good = got == want
        ok += good
        bad += not good
        mark = "ok " if good else "BAD"
        detail = "" if good else "   want %s" % want
        print("  %s %-15s %s%s" % (mark, got, lhs, detail))
    print("LEAN-VERIFY: %s (%d/%d)" % ("PASS" if not bad else "FAIL", ok, len(BATTERY)))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
