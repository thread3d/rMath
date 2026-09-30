"""Unit tests for path/tool resolution (the front end's view of the repo)."""

import os
import tempfile
import unittest
from unittest import mock

from rmathpy import paths


class WorkspaceRoot(unittest.TestCase):
    def test_explicit_override_wins(self):
        with tempfile.TemporaryDirectory() as d:
            with mock.patch.dict(os.environ, {"RMATH_HARNESS_ROOT": d}):
                self.assertEqual(paths.workspace_root(), d)

    def test_falls_back_to_the_repository(self):
        # With no override, no recorded marker and no workdir, the answer is the
        # directory that holds the rmathpy package.
        env = {k: v for k, v in os.environ.items()
               if k not in ("RMATH_HARNESS_ROOT", "RMATH_WORKDIR")}
        with mock.patch.dict(os.environ, env, clear=True):
            with mock.patch.object(paths, "_marker", return_value=""):
                root = paths.workspace_root()
        self.assertTrue(os.path.isdir(os.path.join(root, "rmathpy")))
        self.assertTrue(os.path.isfile(os.path.join(root, "Makefile")))

    def test_workdir_is_used_when_nothing_else_is(self):
        with tempfile.TemporaryDirectory() as d:
            with mock.patch.dict(os.environ, {"RMATH_WORKDIR": d}):
                with mock.patch.object(paths, "_marker", return_value=""):
                    self.assertEqual(paths.workspace_root(), d)


class LeanProject(unittest.TestCase):
    def test_explicit_override(self):
        with tempfile.TemporaryDirectory() as d:
            with mock.patch.dict(os.environ, {"RMATH_LEAN_PROJECT": d}):
                self.assertEqual(paths.lean_project(), d)

    def test_default_is_repo_lean(self):
        with tempfile.TemporaryDirectory() as d:
            with mock.patch.dict(os.environ, {"RMATH_HARNESS_ROOT": d}):
                self.assertEqual(paths.lean_project(), os.path.join(d, "lean"))


class ToolResolution(unittest.TestCase):
    def test_common_bin_dirs_include_home_and_homebrew(self):
        dirs = paths.common_bin_dirs()
        self.assertIn(os.path.expanduser("~/.cargo/bin"), dirs)
        self.assertIn("/opt/homebrew/bin", dirs)

    def test_find_tool_locates_python(self):
        self.assertTrue(paths.find_tool("python3"))

    def test_find_tool_returns_none_for_a_missing_tool(self):
        self.assertIsNone(paths.find_tool("definitely-not-a-real-tool-xyz"))

    def test_child_env_has_a_usable_path(self):
        env = paths.child_env()
        self.assertTrue(env.get("PATH"))
        self.assertIn(os.pathsep, env["PATH"] + os.pathsep)

    def test_preflight_shape(self):
        report = paths.preflight()
        for key in ("node", "dsh", "lake", "problems"):
            self.assertIn(key, report)
        self.assertIsInstance(report["problems"], list)


if __name__ == "__main__":
    unittest.main()
