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
    # integers and lists: ℕ-valued heads, a proposition, and literal lists
    ('GCD[24, 36]', '12', '(Nat.gcd 24 (36)) = (12 : ℕ)', 'ℕ'),
    ('PrimeQ[97]', 'True', '(Nat.Prime 97)', 'ℕ'),
    ('PrimeQ[10]', 'False', '(¬ (Nat.Prime 10))', 'ℕ'),
    ('Range[5]', '{1, 2, 3, 4, 5}',
     '([1, 2, 3, 4, 5] : List ℚ) = ([(1 : ℚ), (2 : ℚ), (3 : ℚ), (4 : ℚ), (5 : ℚ)]'
     ' : List ℚ)', 'ℚ'),
    ('Length[{a, b, c}]', '3', '(([a, b, c] : List ℕ)).length = (3 : ℕ)', 'ℕ'),
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
    # only integer literals carry a faithful translation
    ("GCD[x, 2]", "GCD[x, 2]"),
    ("PrimeQ[1/2]", "False"),
    # sorting needs `native_decide`, which adds a trusted axiom
    ("Sort[{3, 1, 2}]", "{1, 2, 3}"),
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
    # Mathlib has `HasDerivAt.arctan`; the inverse sine and cosine have no such
    # method (they need |x| < 1 and a sqrt side condition), so they stay unwired.
    ("D[ArcTan[x], x]", "1/(1 + x^2)", ".arctan", "Real.arctan"),
]

# Iterated derivatives: D[f, {x, n}] becomes `iteratedDeriv n f x`, proved with
# `iteratedDeriv_pow` (plus const_mul/const), never a first-derivative claim.
ITER_CASES = [
    ("D[x^4, {x, 2}]", "12 x^2", "iteratedDeriv 2"),
    ("D[x^3, {x, 3}]", "6", "iteratedDeriv 3"),
    ("D[3 x^4, {x, 2}]", "36 x^2", "iteratedDeriv_pow"),
    ("D[5, {x, 3}]", "0", "iteratedDeriv"),
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

    for lhs, rhs, want in ITER_CASES:
        try:
            code, prop, _, _ = lean.calculus_statement(lhs, rhs)
        except ValueError as e:
            failures.append("%s: %s" % (lhs, e))
            continue
        if code is None or "iteratedDeriv" not in prop or want not in (prop + code):
            failures.append("%s: unexpected iterated-derivative statement\n     %s\n     %s"
                            % (lhs, code, prop))
    # Display wrappers (Steps/Trace) are transparent to the Lean bridge: a
    # `Steps[...]` cell is checked by the expression it derives, not by the
    # Step list (which has no Lean translation).
    for text, want in [
        ("Steps[D[Sin[x]^2, x]]", "D[Sin[x]^2, x]"),
        ("Trace[1 + 1]", "1 + 1"),
        ("Steps[Solve[x^4 + x^2 - 1 == 0, x]]", "Solve[x^4 + x^2 - 1 == 0, x]"),
        ("D[Sin[x]^2, x]", None),
        ("Steps[]", None),
    ]:
        got = lean.unwrap_display(text)
        if got != want:
            failures.append("unwrap_display(%r) = %r, want %r" % (text, got, want))
    try:
        code, _, _, _ = lean.calculus_statement("D[Sin[x]^2, x]", "2 Cos[x] Sin[x]")
        if code is None or ".pow" not in code:
            failures.append("unwrapped Steps derivative did not translate: %r" % code)
    except ValueError as e:
        failures.append("unwrapped Steps derivative refused: %s" % e)

    # Stage 2 (Lean coverage): indefinite integrals are checked by
    # differentiating the antiderivative, finite sums/produce expand, and
    # Collect-like heads keep only the argument whose value they preserve.
    try:
        _, prop, _, _ = lean.calculus_statement("Integrate[x^2, x]", "1/3 x^3")
        if "deriv" not in prop:
            failures.append("antiderivative statement has no deriv: %s" % prop)
    except ValueError as e:
        failures.append("antiderivative refused: %s" % e)
    for lhs, rhs, want in [("Sum[i, {i, 1, 4}]", "10", " + "),
                           ("Product[i, {i, 1, 4}]", "24", " * "),
                           ("Collect[x + x^2, x]", "x + x^2", "+")]:
        _, prop, _, _ = lean.statement(lhs, rhs)
        if want not in prop:
            failures.append("%s did not translate as expected: %s" % (lhs, prop))

    # Solve roots are checked by substitution; non-real roots are dropped
    # (Lean's `Real.sqrt` of a negative is 0, so those would be false claims).
    for solve_text, roots, numeric, want in [
        ("Solve[x^2 - 3 x + 2 == 0, x]", "{{x -> 2}, {x -> 1}}",
         "{{x -> 2.0}, {x -> 1.0}}", 2),
        ("Solve[x^4 + x^2 - 1 == 0, x]",
         "{{x -> Sqrt[1/2 (-1 + Sqrt[5])]}, {x -> -Sqrt[1/2 (-1 + Sqrt[5])]}, "
         "{x -> Sqrt[1/2 (-1 - Sqrt[5])]}, {x -> -Sqrt[1/2 (-1 - Sqrt[5])]}}",
         "{{x -> 0.786151}, {x -> -0.786151}, "
         "{x -> 7.78887*^-17 + 1.27202 I}, {x -> -7.78887*^-17 + -1.27202 I}}", 2),
    ]:
        code, prop, _, _ = lean.solve_statement(solve_text, roots, numeric=numeric)
        got = prop.count("\u2227") + 1
        if got != want:
            failures.append("%s: %d substituted roots, want %d" % (solve_text, got, want))
    if lean.solve_statement("1 + 1", "{{x -> 2}}") is not None:
        failures.append("solve_statement on a non-Solve input should return None")

    # Bug report 20260927-163001: OutputForm juxtaposition without spaces
    # (`2x^2-2`) is valid rMath but is not valid Python as written.
    for text, want in [
        ("2x^2 - 2", "2*x**2 - 2"),
        ("Factor[2x^2-2]", "(2*x**2-2)"),
        ("2(x+1)(x-1)", "2*(x+1)*(x-1)"),
        ("Log2[x]", "Log2(x)"),
        ("x2", "x2"),
    ]:
        got = lean._rmath_to_python(text)
        if got != want:
            failures.append("_rmath_to_python(%r) = %r, want %r" % (text, got, want))
    # all-complex roots are checked over ℂ (backlog item 3), not refused.
    _, prop, ty, _ = lean.solve_statement("Solve[x^2 + 1 == 0, x]",
                                          "{{x -> I}, {x -> -I}}",
                                          numeric="{{x -> 1.0 I}, {x -> -1.0 I}}")
    if ty != "\u2102" or "Complex.I" not in prop:
        failures.append("Solve[x^2 + 1 == 0, x]: expected a \u2102 claim, got %s / %s"
                        % (ty, prop))

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
