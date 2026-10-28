"""Unit tests for the Lean bridge's translation layer.

These exercise the pure translation/plumbing code: no Lean, no Mathlib and no
network are needed. `make lean-test` runs the slower end-to-end translation
battery; this file pins the individual helpers it depends on.
"""

import ast
import os
import unittest
from unittest import mock

from rmathpy import lean


class StatementTranslation(unittest.TestCase):
    def test_equality_of_rationals_is_typed_q(self):
        code, prop, ty, vs = lean.statement("1/2 + 1/3", "5/6")
        self.assertEqual(ty, "ℚ")
        self.assertEqual(vs, [])
        self.assertIn("(1 : ℚ)", prop)
        self.assertIn("(5 : ℚ)", prop)
        self.assertIn("example", code)
        self.assertIn("solve |", code)

    def test_real_functions_force_the_real_type(self):
        _, prop, ty, _ = lean.statement("Sqrt[2]^2", "2")
        self.assertEqual(ty, "ℝ")
        self.assertIn("Real.sqrt", prop)

    def test_free_variables_are_collected_and_bound(self):
        code, prop, ty, vs = lean.statement("Factor[x^2 - 1]", "(-1 + x) (1 + x)")
        self.assertEqual(vs, ["x"])
        self.assertEqual(ty, "ℚ")
        self.assertIn("(x : ℚ)", code)
        self.assertIn("x ^ (2 : ℕ)", prop)

    def test_a_claim_without_a_rhs_needs_to_be_a_comparison(self):
        _, prop, _, vs = lean.statement("x^2 >= 0")
        self.assertEqual(vs, ["x"])
        self.assertIn("≥", prop)

    def test_a_bare_expression_is_refused_as_a_claim(self):
        with self.assertRaises(ValueError):
            lean.statement("Sin[x]")

    def test_unparseable_input_is_refused(self):
        with self.assertRaises(ValueError):
            lean.statement("Sin[x", "0")


class TextHelpers(unittest.TestCase):
    def test_rmath_to_python_rewrites_powers_and_implicit_times(self):
        self.assertEqual(lean._rmath_to_python("x^2"), "x**2")
        self.assertEqual(lean._rmath_to_python("2 x"), "2*x")
        self.assertEqual(lean._rmath_to_python("x y^3"), "x*y**3")
        self.assertEqual(lean._rmath_to_python("Sin[x] + 1"), "Sin(x) + 1")
        self.assertEqual(lean._rmath_to_python("1/2"), "1/2")

    def test_to_text_uses_the_ast_not_the_raw_string(self):
        node = ast.parse("x + 1", mode="eval").body
        self.assertEqual(lean._to_text(node), "x + 1")

    def test_split_top_level_respects_brackets(self):
        self.assertEqual(lean._split_top_level("a, b, c[d, e]"), ["a", "b", "c[d, e]"])
        self.assertEqual(lean._split_top_level("just one"), ["just one"])

    def test_balanced_brackets(self):
        self.assertTrue(lean._balanced_brackets("(a[b])"))
        self.assertFalse(lean._balanced_brackets("(a[b]"))

    def test_expand_finite_sum_becomes_explicit_terms(self):
        self.assertEqual(lean._expand_finite("Sum[i, {i, 1, 3}]"), "((1) + (2) + (3))")
        # A symbolic bound is left alone.
        self.assertEqual(lean._expand_finite("Sum[i, {i, 1, n}]"), "Sum[i, {i, 1, n}]")

    def test_transparent_unwrap_reduces_display_wrappers(self):
        self.assertEqual(lean._transparent_unwrap("Collect[x^2 + x, x]"), "x^2 + x")
        self.assertEqual(lean._transparent_unwrap("x^2 + x"), "x^2 + x")

    def test_unwrap_display(self):
        self.assertEqual(lean.unwrap_display("Steps[D[Sin[x]^2, x]]"), "D[Sin[x]^2, x]")
        self.assertIsNone(lean.unwrap_display("D[Sin[x]^2, x]"))

    def test_is_solve(self):
        self.assertTrue(lean.is_solve("Solve[x^2 == 1, x]"))
        self.assertFalse(lean.is_solve("x^2 == 1"))

    def test_root_texts(self):
        self.assertEqual(lean._root_texts("{{x -> 2}, {x -> 1}}"), ["2", "1"])


class CalculusStatements(unittest.TestCase):
    def test_definite_integral_leaves_have_an_integral_translation(self):
        code, prop, ty, _ = lean.calculus_statement("Integrate[x^2, {x, 0, 1}]", "1/3")
        self.assertEqual(ty, "ℝ")
        self.assertIn("∫", prop)
        self.assertIn("integral_pow", code)

    def test_derivative_is_wired_through_hasderivat(self):
        code, prop, ty, _ = lean.calculus_statement("D[x^3, x]", "3 x^2")
        self.assertEqual(ty, "ℝ")
        self.assertIn("deriv (fun x : ℝ =>", prop)
        self.assertIn("hasDerivAt_pow", code)

    def test_derivative_helper_produces_a_deriv_statement(self):
        _, prop, ty, _ = lean.derivative_statement("D[Sin[x]^2, x]", "2 Cos[x] Sin[x]", "x")
        self.assertEqual(ty, "ℝ")
        self.assertIn("deriv", prop)
        self.assertIn("Real.sin", prop)

    def test_iterated_derivative_uses_iteratedderiv(self):
        code, prop, ty, _ = lean.calculus_statement("D[x^4, {x, 2}]", "12 x^2")
        self.assertEqual(ty, "ℝ")
        self.assertIn("iteratedDeriv 2", prop)
        self.assertIn("iteratedDeriv_pow", code)

    def test_iterated_derivative_is_recognised_by_calculus(self):
        found = lean._calculus("D[x^4, {x, 2}]")
        self.assertIsNotNone(found)
        kind, _target, var, n, _ = found
        self.assertEqual((kind, var, n), ("derivative_n", "x", 2))
        # `n` must be a positive integer literal.
        self.assertIsNone(lean._calculus("D[x^4, {x, n}]"))

    def test_none_for_a_non_calculus_claim(self):
        self.assertIsNone(lean.calculus_statement("1/2 + 1/3", "5/6"))


class SolveStatements(unittest.TestCase):
    def test_solve_checks_every_root_by_substitution(self):
        code, prop, _, _ = lean.solve_statement(
            "Solve[x^2 - 3 x + 2 == 0, x]", "{{x -> 1}, {x -> 2}}")
        self.assertIn("∧", prop)          # both roots are asserted
        self.assertIn("(1 : ℚ)", prop)
        self.assertIn("(2 : ℚ)", prop)
        self.assertIn("solve |", code)

    def test_build_statement_prefers_calculus_then_solve(self):
        _, prop, ty, _ = lean.build_statement("Integrate[x^2, {x, 0, 1}]", "1/3")
        self.assertIn("∫", prop)
        _, prop, _, _ = lean.build_statement(
            "Solve[x^2 - 3 x + 2 == 0, x]",
            result_form="{{x -> 1}, {x -> 2}}")
        self.assertIn("∧", prop)


class DiagnosticsAndAudit(unittest.TestCase):
    def test_parse_diagnostics_folds_continuation_lines(self):
        # `lean` prints a diagnostic header and then the goal at column 0.
        out = ("RMathCheck.lean:12:3: error: unsolved goals\n"
               "x : ℝ\n"
               "⊢ x = 2\n"
               "RMathCheck.lean:20:1: warning: declaration uses 'sorry'\n")
        diags = lean.parse_diagnostics(out)
        self.assertEqual(len(diags), 2)
        self.assertEqual(diags[0]["severity"], "error")
        self.assertEqual(diags[0]["line"], 12)
        self.assertIn("x = 2", diags[0]["message"])
        self.assertEqual(diags[1]["severity"], "warning")

    def test_parse_axioms(self):
        self.assertEqual(
            lean._parse_axioms("'rmath_check' depends on axioms: [propext, sorryAx]"),
            ["propext", "sorryAx"])
        self.assertEqual(lean._parse_axioms("no axioms here"), [])

    def test_sorry_is_recognised_as_unproved(self):
        # The verdict logic downgrades a sorry proof; check the marker it looks
        # for is the one Lean actually emits.
        self.assertIn("sorryAx", lean._parse_axioms(
            "depends on axioms: [sorryAx]"))

    def test_named_and_display_turn_example_into_an_audited_theorem(self):
        named = lean._named("example : (1 : ℚ) = 1 := by norm_num")
        self.assertIn("theorem rmath_check", named)
        self.assertNotIn("example", named.split("theorem")[0])
        display = lean._display(
            lean.QUIET + "example : (1 : ℚ) = 1 := by norm_num")
        self.assertIn("#print axioms rmath_check", display)
        self.assertNotIn("linter.unusedTactic", display)

    def test_persistent_mode_env(self):
        for value, expected in [("0", "off"), ("off", "off"), ("1", "require"),
                                ("require", "require"), ("", "auto"), ("auto", "auto")]:
            with mock.patch.dict(os.environ, {"RMATH_LEAN_SERVER": value}):
                self.assertEqual(lean.persistent_mode(), expected, value)

    def test_premise_search_rejects_an_unknown_backend_without_network(self):
        res = lean.premise_search("x", backend="nope")
        self.assertEqual(res["status"], "error")
        self.assertEqual(res["results"], [])


if __name__ == "__main__":
    unittest.main()


class ToolchainDiscovery(unittest.TestCase):
    """Where the bridge looks for Lean, and when it claims to be usable.

    A Lean installation without Mathlib parses `import Mathlib` as an unknown
    module, so `lake` on PATH is not enough — the bridge used to report
    "available" on a machine where every check failed.
    """

    def test_elan_bin_comes_from_the_environment_first(self):
        with mock.patch.dict(os.environ, {"ELAN_BIN": "/opt/elan/bin"}, clear=False):
            self.assertEqual(lean.elan_bin(), "/opt/elan/bin")
        with mock.patch.dict(os.environ, {"ELAN_HOME": "/opt/elan-home"}, clear=False):
            os.environ.pop("ELAN_BIN", None)
            self.assertEqual(lean.elan_bin(), os.path.join("/opt/elan-home", "bin"))

    def test_a_repository_local_elan_is_found(self):
        env = {k: v for k, v in os.environ.items() if k not in ("ELAN_BIN", "ELAN_HOME")}
        local = os.path.join(os.path.dirname(lean.project_dir()), ".elan", "bin")
        with mock.patch.dict(os.environ, env, clear=True), \
                mock.patch("os.path.isfile", lambda p: p == os.path.join(local, "lake")):
            self.assertEqual(lean.elan_bin(), local)
            self.assertEqual(lean.lake(), os.path.join(local, "lake"))

    def test_the_elan_shim_beats_a_system_lake(self):
        """A system lake of another version cannot build the pinned Mathlib."""
        with mock.patch("os.path.isfile", lambda p: p.endswith("lake")), \
                mock.patch("shutil.which", lambda name: "/usr/local/bin/" + name):
            self.assertNotEqual(lean.lake(), "/usr/local/bin/lake")

    def test_without_elan_the_system_lake_is_used(self):
        with mock.patch("os.path.isfile", lambda p: False), \
                mock.patch("shutil.which", lambda name: "/usr/local/bin/" + name):
            self.assertEqual(lean.lake(), "/usr/local/bin/lake")

    def test_the_run_environment_puts_the_shims_first(self):
        with mock.patch("os.path.isfile", lambda p: p.endswith("lake")):
            env = lean.lean_env()
        first = env["PATH"].split(os.pathsep)[0]
        self.assertTrue(first.endswith(os.path.join(".elan", "bin")), first)
        self.assertTrue(env.get("ELAN_HOME", "").endswith(".elan"), env.get("ELAN_HOME"))

    def test_available_needs_mathlib_not_just_lake(self):
        with mock.patch.object(lean, "lake", lambda: "/usr/local/bin/lake"), \
                mock.patch.object(lean, "mathlib_olean", lambda: None):
            self.assertFalse(lean.available())
        with mock.patch.object(lean, "lake", lambda: "/usr/local/bin/lake"), \
                mock.patch.object(lean, "mathlib_olean", lambda: "/x/Mathlib.olean"):
            self.assertTrue(lean.available())
        with mock.patch.object(lean, "lake", lambda: None), \
                mock.patch.object(lean, "mathlib_olean", lambda: "/x/Mathlib.olean"):
            self.assertFalse(lean.available())

    def test_mathlib_olean_points_at_the_built_module(self):
        path = os.path.join(lean.project_dir(), ".lake", "packages", "mathlib",
                            ".lake", "build", "lib", "lean", "Mathlib.olean")
        with mock.patch("os.path.isfile", lambda p: p == path):
            self.assertEqual(lean.mathlib_olean(), path)
        with mock.patch("os.path.isfile", lambda p: False):
            self.assertIsNone(lean.mathlib_olean())
