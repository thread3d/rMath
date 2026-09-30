"""Unit tests for the harness configuration helpers (no agent runs, no network)."""

import os
import stat
import tempfile
import unittest
from unittest import mock

from rmathpy import harness


class Endpoint(unittest.TestCase):
    def test_environment_wins(self):
        with mock.patch.dict(os.environ, {"DEEPSEEK_BASE_URL": "http://example.invalid/v1"}):
            url, source = harness.endpoint()
        self.assertEqual(url, "http://example.invalid/v1")
        self.assertEqual(source, "environment")


class Keyfile(unittest.TestCase):
    def test_keyfile_override(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "harness.env")
            with mock.patch.dict(os.environ, {"RMATH_HARNESS_KEYFILE": path}):
                self.assertEqual(harness.keyfile(), path)

    def test_store_and_read_key_round_trip(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "harness.env")
            env = {"RMATH_HARNESS_KEYFILE": path, "DEEPSEEK_API_KEY": "",
                   "DEEPSEEK_BASE_URL": "http://example.invalid/v1"}
            with mock.patch.dict(os.environ, env):
                harness.store_key("sk-secret", "http://example.invalid/v1")
                self.assertEqual(harness.read_key(), "sk-secret")
                # The stored file is not world-readable.
                mode = stat.S_IMODE(os.stat(path).st_mode)
                self.assertEqual(mode, 0o600)
                env_out = harness.read_env()
                self.assertEqual(env_out["DEEPSEEK_API_KEY"], "sk-secret")

    def test_environment_key_wins_over_the_file(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "harness.env")
            with open(path, "w") as f:
                f.write("DEEPSEEK_API_KEY=from-file\n")
            env = {"RMATH_HARNESS_KEYFILE": path, "DEEPSEEK_API_KEY": "from-env",
                   "DEEPSEEK_BASE_URL": "http://example.invalid/v1"}
            with mock.patch.dict(os.environ, env):
                self.assertEqual(harness.read_key(), "from-env")

    def test_a_file_without_the_key_line_yields_no_key(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "harness.env")
            with open(path, "w") as f:
                f.write("SOMETHING_ELSE=value\n")
            env = {"RMATH_HARNESS_KEYFILE": path, "DEEPSEEK_API_KEY": "",
                   "DEEPSEEK_BASE_URL": "http://example.invalid/v1"}
            with mock.patch.dict(os.environ, env):
                self.assertEqual(harness.read_key(), "")


class ModelChoice(unittest.TestCase):
    def test_environment_override(self):
        with mock.patch.dict(os.environ, {"RMATH_HARNESS_MODEL": "m1"}):
            self.assertEqual(harness.read_model(), "m1")
            self.assertEqual(harness.harness_model(), "m1")

    def test_file_round_trip_and_clear(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "model")
            env = {"RMATH_HARNESS_MODEL_FILE": path, "RMATH_HARNESS_MODEL": ""}
            with mock.patch.dict(os.environ, env):
                harness.store_model("ornith-1.5:35b")
                self.assertEqual(harness.read_model(), "ornith-1.5:35b")
                harness.store_model("")
                self.assertEqual(harness.read_model(), "")

    def test_settings_document_is_parsed_without_yaml(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "settings.yaml")
            with open(path, "w") as f:
                f.write("# a comment\n"
                        "agent-default-model:\n"
                        "  provider: deepseek-official\n"
                        "  model: ornith-1.5:35b   # inline comment\n"
                        "other-key:\n"
                        "  model: ignore-me\n")
            with mock.patch.dict(os.environ, {"RMATH_DSH_SETTINGS": path}):
                self.assertEqual(harness.settings_model(), "ornith-1.5:35b")

    def test_missing_settings_file_is_empty(self):
        with mock.patch.dict(os.environ,
                             {"RMATH_DSH_SETTINGS": "/nonexistent/settings.yaml"}):
            self.assertEqual(harness.settings_model(), "")


class ProfileAndTasks(unittest.TestCase):
    def test_profile_default_and_override(self):
        env = {k: v for k, v in os.environ.items() if k != "RMATH_DSH_PROFILE"}
        with mock.patch.dict(os.environ, env, clear=True):
            self.assertEqual(harness.profile(), "headless")
        with mock.patch.dict(os.environ, {"RMATH_DSH_PROFILE": "acp"}):
            self.assertEqual(harness.profile(), "acp")

    def test_current_is_none_when_idle(self):
        with mock.patch.object(harness, "_CURRENT",
                               {"proc": None, "started": None, "task": ""}):
            self.assertIsNone(harness.current())

    def test_cell_task_carries_the_cell(self):
        task = harness.cell_task("Sin[x]", "Sin[x]", "some::msg")
        self.assertIn("Sin[x]", task)
        self.assertIn("some::msg", task)

    def test_fix_task_names_the_report(self):
        task = harness.fix_task("/tmp/bug-1.md")
        self.assertIn("/tmp/bug-1.md", task)
        self.assertIn("make test", task)


if __name__ == "__main__":
    unittest.main()
