"""Tests for the demo validator's re-spelling check.

`make demo-gen` exists so an example that stops evaluating is reported instead
of being left in demo/demo.mt as a lie. Comparing the printed result with the
input string is not enough: an unevaluated builtin re-prints in canonical
order, so `Sin[Pi/6]` came back as `Sin[1/6 Pi]` and passed. These tests pin the
helpers that close that hole.
"""

import importlib.util
import os
import unittest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SPEC = importlib.util.spec_from_file_location(
    "demo_gen", os.path.join(ROOT, "scripts", "dev", "demo_gen.py"))
demo_gen = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(demo_gen)


class StubKernel:
    """Answers the one question is_respelling asks: is this head a builtin?"""

    def __init__(self, builtins):
        self.builtins = builtins

    def eval(self, text):
        # text is MemberQ[Attributes[<head>], Protected]
        head = text.split("Attributes[", 1)[1].split("]", 1)[0]
        return {"status": "ok", "result": "True" if head in self.builtins else "False"}


class HeadOf(unittest.TestCase):
    def test_call_forms(self):
        self.assertEqual(demo_gen.head_of("Sin[Pi/6]"), "Sin")
        self.assertEqual(demo_gen.head_of("  Chop[0.0000001]  "), "Chop")
        self.assertEqual(demo_gen.head_of("$Version"), None)

    def test_operator_forms_have_no_written_head(self):
        self.assertEqual(demo_gen.head_of("x^m x^n"), None)
        self.assertEqual(demo_gen.head_of("1 + 2"), None)
        self.assertEqual(demo_gen.head_of("3 I"), None)


class TopLevelArity(unittest.TestCase):
    def test_counts_arguments(self):
        self.assertEqual(demo_gen.top_level_arity("Sin[Pi/6]"), 1)
        self.assertEqual(demo_gen.top_level_arity("Chop[0.1, 0.01]"), 2)
        self.assertEqual(demo_gen.top_level_arity("f[g[1, 2], 3]"), 2)
        self.assertEqual(demo_gen.top_level_arity("Solve[x^2 == 1, x]"), 2)

    def test_respects_strings_and_nesting(self):
        self.assertEqual(demo_gen.top_level_arity('f["a,b", c]'), 2)
        self.assertEqual(demo_gen.top_level_arity('f["a\\",b"]'), 1)

    def test_atoms_and_malformed_text(self):
        self.assertEqual(demo_gen.top_level_arity("x"), None)
        self.assertEqual(demo_gen.top_level_arity("f[1, 2"), None)


class Respelling(unittest.TestCase):
    def setUp(self):
        demo_gen._BUILTIN_CACHE.clear()

    def test_builtin_keeping_its_head_is_a_respelling(self):
        k = StubKernel({"Sin", "Chop", "Cot"})
        self.assertTrue(demo_gen.is_respelling(k, "Sin[Pi/6]", "Sin[1/6 Pi]"))
        self.assertTrue(demo_gen.is_respelling(k, "Chop[0.0000001]", "Chop[1.0*^-7]"))

    def test_a_real_answer_is_not(self):
        k = StubKernel({"Sin", "Chop", "Cot"})
        self.assertFalse(demo_gen.is_respelling(k, "Sin[Pi/6]", "1/2"))
        self.assertFalse(demo_gen.is_respelling(k, "Chop[0.0000001]", "1.0*^-7"))

    def test_a_user_head_is_the_definitions_own_answer(self):
        k = StubKernel({"Sin"})
        self.assertFalse(demo_gen.is_respelling(k, "f[a; b]", "f[b]"))

    def test_arity_change_is_a_real_evaluation(self):
        k = StubKernel({"Sin"})
        self.assertFalse(demo_gen.is_respelling(k, "Sin[x, y]", "Sin[y]"))

    def test_allowlisted_heads_are_skipped(self):
        k = StubKernel({"Power", "Hold"})
        self.assertFalse(demo_gen.is_respelling(k, "Power[x, 2]", "Power[x, 2]"))
        self.assertFalse(demo_gen.is_respelling(k, "Hold[1 + 1]", "Hold[1 + 1]"))


if __name__ == "__main__":
    unittest.main()
