#!/usr/bin/env python3
"""Translation tests for the Lean bridge (no Lean required).

Run: python3 scripts/dev/lean_translation_test.py
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
from rmathpy import lean  # noqa: E402

CASES = [
    ('1/2 + 1/3', '5/6',
     '(((1 : ℚ) / (2 : ℚ)) + ((1 : ℚ) / (3 : ℚ))) = ((5 : ℚ) / (6 : ℚ))', 'ℚ'),
    ('2^10', '1024',
     '((2 : ℚ) ^ (10 : ℕ)) = (1024 : ℚ)', 'ℚ'),
    ('Sqrt[2]^2', '2',
     '(Real.sqrt ((2 : ℝ)) ^ (2 : ℕ)) = (2 : ℝ)', 'ℝ'),
    ('Abs[-7]', '7',
     'abs ((-(7 : ℚ))) = (7 : ℚ)', 'ℚ'),
    ('Factor[x^2 - 1]', '(-1 + x) (1 + x)',
     '((x ^ (2 : ℕ)) - (1 : ℚ)) = (((-(1 : ℚ)) + x) * ((1 : ℚ) + x))', 'ℚ'),
    ('Expand[(x + 1)^3]', '1 + 3 x + 3 x^2 + x^3',
     '((x + (1 : ℚ)) ^ (3 : ℕ)) = ((((1 : ℚ) + ((3 : ℚ) * x)) + ((3 : ℚ) * (x ^ (2 : ℕ)))) + (x ^ (3 : ℕ)))', 'ℚ'),
    ('Rational[3, 4]', '3/4',
     '(((3 : ℚ)) / ((4 : ℚ)) : ℚ) = ((3 : ℚ) / (4 : ℚ))', 'ℚ'),
    ('Sin[Pi/6]', '1/2',
     'Real.sin ((Real.pi / (6 : ℝ))) = ((1 : ℝ) / (2 : ℝ))', 'ℝ'),
    ('x^2 + 2 x + 1 == (x + 1)^2', None,
     '(((x ^ (2 : ℕ)) + ((2 : ℚ) * x)) + (1 : ℚ)) = ((x + (1 : ℚ)) ^ (2 : ℕ))', 'ℚ'),
]

REFUSALS = [
    # calculus goes through its own path in lean.verify
    ("Integrate[x^2, {x, 0, 1}]", "1/3"),
    ("D[x^3 + Sin[x], x]", None),
    # a bare expression is not a claim
    ("20!", None),
    ("x^2 + 2 x + 1", None),
    ("N[Pi]", "3.14159"),
    ("2 I", "0 + 2 I"),
    ("GCD[24, 36]", "12"),
]


# Derivative proof-term shapes (generated without running Lean).
DERIV_CASES = [
    ("D[x, x]", "1", "hasDerivAt_id x", "deriv id x"),
    ("D[7, x]", "0", "hasDerivAt_const", "deriv (fun _"),
    ("D[Cos[x], x]", "-Sin[x]", ".cos", "Real.cos"),
    ("D[Exp[x^2], x]", "2 x Exp[x^2]", ".exp", "Real.exp"),
    ("D[Sin[x]^2, x]", "2 Cos[x] Sin[x]", ".pow", "Real.sin"),
    ("D[x^3 + Sin[x], x]", "Cos[x] + 3 x^2", ".add", "Real.cos"),
    ("D[3 x^4, x]", "12 x^3", ".const_mul", "deriv (fun x"),
    ("D[1/x, x]", "-1/x^2", ".div", "deriv", "(x ≠ 0)"),
    ("D[x^-1, x]", "-1/x^2", ".inv", "deriv", "(x ≠ 0)"),
    ("D[Sin[x]/x, x]", "Cos[x]/x - 1/x^2 Sin[x]", ".div", "deriv", "(x ≠ 0)"),
    ("D[Log[x], x]", "1/x", ".log", "Real.log", "(x ≠ 0)"),
    ("D[Sqrt[x], x]", "1/2 1/Sqrt[x]", ".sqrt", "Real.sqrt", "(x ≠ 0)"),
]


def main():
    failures = []
    for lhs, rhs, want_prop, want_ty in CASES:
        try:
            _, prop, ty, _ = lean.statement(lhs, rhs)
        except ValueError as e:
            failures.append("%s: refused (%s)" % (lhs, e))
            continue
        if prop != want_prop or ty != want_ty:
            failures.append("%s\n     got  %s  [%s]\n     want %s  [%s]"
                            % (lhs, prop, ty, want_prop, want_ty))
    for lhs, rhs in REFUSALS:
        try:
            lean.statement(lhs, rhs)
            failures.append("%s: expected a refusal, got a translation" % lhs)
        except ValueError:
            pass

    for case in DERIV_CASES:
        lhs, rhs, term_part, prop_part = case[:4]
        binder_part = case[4] if len(case) > 4 else ""
        try:
            code, prop, ty, _ = lean.calculus_statement(lhs, rhs)
        except ValueError as e:
            failures.append("%s: %s" % (lhs, e))
            continue
        if (code is None or term_part not in code or prop_part not in prop
                or (binder_part and binder_part not in code)):
            failures.append("%s: unexpected proof term/proposition\n     %s\n     %s"
                            % (lhs, code, prop))
    # a quotient-shaped claim with no side condition available must still refuse
    try:
        lean.calculus_statement("D[Abs[x], x]", "1")
        failures.append("D[Abs[x], x]: expected a refusal (no HasDerivAt lemma for abs)")
    except ValueError:
        pass

    if failures:
        print("LEAN TRANSLATION: FAIL")
        for f in failures:
            print("  " + f)
        return 1
    print("LEAN TRANSLATION: PASS (%d claims, %d derivative shapes, %d deliberate refusals)"
          % (len(CASES), len(DERIV_CASES), len(REFUSALS) + 1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
