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

    if failures:
        print("LEAN TRANSLATION: FAIL")
        for f in failures:
            print("  " + f)
        return 1
    print("LEAN TRANSLATION: PASS (%d cases, %d deliberate refusals)"
          % (len(CASES) - 1, len(REFUSALS)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
