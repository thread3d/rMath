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
    # indefinite integrals: proved by differentiating the antiderivative
    ("Integrate[x^2, x]", "1/3 x^3", "verified"),
    ("Integrate[Sin[x], x]", "-Cos[x]", "verified"),
    ("Integrate[Exp[x], x]", "Exp[x]", "verified"),
    # Solve, including nested radicals closed by `rmath_radical`
    ("Solve[x^2 - 2 == 0, x]", "{{x -> Sqrt[2]}, {x -> -Sqrt[2]}}", "verified"),
    ("Solve[x^4 - 5 x^2 + 4 == 0, x]",
     "{{x -> 1}, {x -> -1}, {x -> 2}, {x -> -2}}", "verified"),
    ("Solve[x^4 + x^2 - 1 == 0, x]",
     "{{x -> Sqrt[1/2 (-1 + Sqrt[5])]}, {x -> -Sqrt[1/2 (-1 + Sqrt[5])]}, "
     "{x -> Sqrt[1/2 (-1 - Sqrt[5])]}, {x -> -Sqrt[1/2 (-1 - Sqrt[5])]}}",
     "verified"),
    # matrices: a literal is `!![...]` over ℚ, and the tactics close the entries
    ("Det[{{2, 1}, {1, 3}}]", "5", "verified"),
    ("Transpose[{{1, 2}, {3, 4}}]", "{{1, 3}, {2, 4}}", "verified"),
    ("Dot[{{1, 2}, {3, 4}}, {5, 6}]", "{17, 39}", "verified"),
    ("Dot[{5, 6}, {{1, 2}, {3, 4}}]", "{23, 34}", "verified"),
    ("Dot[{1, 2}, {3, 4}]", "11", "verified"),
    # inverse-tangent wiring: the derivative, and the two integrals it unlocks
    ("D[ArcTan[x], x]", "1/(1 + x^2)", "verified"),
    ("Integrate[1/(1 + x^2), x]", "ArcTan[x]", "verified"),
    ("Integrate[1/(4 + x^2), x]", "1/2 ArcTan[1/2 x]", "verified"),
    # integers and lists: Nat.gcd, Nat.Prime, List.length, literal lists
    ("GCD[24, 36]", "12", "verified"),
    ("GCD[2^64, 2^70]", "18446744073709551616", "verified"),
    ("PrimeQ[97]", "True", "verified"),
    ("Range[5]", "{1, 2, 3, 4, 5}", "verified"),
    ("Length[{a, b, c}]", "3", "verified"),
    # must NOT be accepted
    ("1/2 + 1/3", "2/3", "unproved"),
    ("2 + 2", "5", "unproved"),
    ("x^2 + 2 x + 1", "0", "unproved"),
    # must be refused with a reason, not guessed at
    ("N[Pi]", "3.14159", "cannot verify"),
    ("2 I", "0 + 2 I", "cannot verify"),
    ("PrimeQ[1/2]", "False", "cannot verify"),
]

# `N[roots]` for a Solve case: the bridge drops the roots with an imaginary part.
# `Solve[x^4+x^2-1==0, x]` has two real roots and two non-real ones.
NUMERIC = {
    "Solve[x^4 + x^2 - 1 == 0, x]":
        "{{x -> 0.786151}, {x -> -0.786151}, "
        "{x -> 7.78887*^-17 + 1.27202 I}, {x -> -7.78887*^-17 + -1.27202 I}}",
}


def main():
    if not lean.available():
        # A silent skip is how "Lean is installed" stays false for months: the
        # suite goes green without ever checking a proof. Say what is missing
        # and how to fix it, and let the exit status carry the bad news.
        st = lean.status()
        print("LEAN-VERIFY: NOT READY — %s" % (st["next_step"] or "Lean is unavailable"))
        print("  (add --skip to run nothing and exit 0, e.g. on a machine without Lean)")
        return 1 if "--skip" not in sys.argv else 0
    ok = bad = 0
    for lhs, rhs, want in BATTERY:
        result_form = rhs if lean.is_solve(lhs) else None
        r = lean.verify(lhs, rhs, timeout=600, result_form=result_form,
                        numeric=NUMERIC.get(lhs))
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
