"""Unit tests for the notebook front end's pure helpers and HTTP routing."""

import json
import os
import threading
import unittest
import urllib.error
import urllib.request
from http.server import ThreadingHTTPServer
from unittest import mock

from rmathpy import notebook
from rmathpy.notebook import Handler


class Balanced(unittest.TestCase):
    def test_balanced_brackets(self):
        self.assertTrue(notebook._balanced("f[a, {b, c}]"))
        self.assertTrue(notebook._balanced(""))
        self.assertFalse(notebook._balanced("f[a"))
        self.assertFalse(notebook._balanced("f[a]]"))

    def test_strings_are_skipped(self):
        self.assertTrue(notebook._balanced('"["'))
        self.assertTrue(notebook._balanced('"\\"]"'))
        self.assertFalse(notebook._balanced('"unterminated'))


class DemoExpression(unittest.TestCase):
    def test_labelled_print_becomes_the_bare_expression(self):
        self.assertEqual(
            notebook.demo_expression('Print["  1/2 + 1/3  =  ", 1/2 + 1/3]'),
            "1/2 + 1/3")
        self.assertEqual(
            notebook.demo_expression('Print["  f[1]  =  ", f[x, y]]'),
            "f[x, y]")

    def test_a_print_without_the_label_convention_is_kept(self):
        self.assertEqual(notebook.demo_expression('Print["hello"]'),
                         'Print["hello"]')
        self.assertEqual(notebook.demo_expression("x + 1"), "x + 1")

    def test_a_continuation_print_line_is_kept(self):
        line = 'Print["          more prose, no expression"]'
        self.assertEqual(notebook.demo_expression(line), line)

    def test_unbalanced_body_is_kept(self):
        line = 'Print["  a  =  ", f[x]'
        self.assertEqual(notebook.demo_expression(line), line)


class FuzzClaim(unittest.TestCase):
    class Fake:
        def __init__(self, results):
            self.results = list(results)
            self.i = 0

        def eval(self, text, steps=False):
            r = self.results[min(self.i, len(self.results) - 1)]
            self.i += 1
            return {"status": "ok", "result": r}

    def test_agreement_is_silent(self):
        k = self.Fake(["1", "1", "1", "1"])
        self.assertEqual(notebook.fuzz_claim(k, "x^2", "x x", ["x"], samples=2), "")

    def test_disagreement_is_reported(self):
        k = self.Fake(["1", "2"])
        self.assertNotEqual(notebook.fuzz_claim(k, "x^2", "x", ["x"], samples=1), "")

    def test_no_variables_or_no_rhs_is_not_checked(self):
        k = self.Fake(["1"])
        self.assertEqual(notebook.fuzz_claim(k, "x", "x", [], samples=3), "")
        self.assertEqual(notebook.fuzz_claim(k, "x", "", ["x"], samples=3), "")

    def test_a_kernel_error_is_not_checked(self):
        class Bad(self.Fake):
            def eval(self, text, steps=False):
                return {"status": "error", "error": "boom"}

        self.assertEqual(notebook.fuzz_claim(Bad(["1"]), "x", "x", ["x"]), "")


class WorkDir(unittest.TestCase):
    def test_work_dir_honours_the_env_override(self):
        with mock.patch.dict(os.environ, {"RMATH_WORKDIR": "/tmp/rmath-test"}):
            self.assertEqual(notebook.work_dir(), "/tmp/rmath-test")
            self.assertEqual(notebook.session_path(),
                             "/tmp/rmath-test/.rmath-session.json")


class FakeKernel:
    """A stand-in for the Rust kernel client (no subprocess)."""

    def __init__(self):
        self.calls = []

    def eval(self, text, steps=False):
        self.calls.append(("eval", text, steps))
        if text == "Plot[Sin[x], {x, 0, 1}]":
            return {"status": "ok", "result": "Graphics[...]",
                    "graphics": {"type": "Graphics",
                                 "primitives": [{"type": "Line",
                                                 "pts": [[0, 0], [1, 1]]}],
                                 "plotrange": [0, 1, 0, 1]}}
        return {"status": "ok", "result": "42", "out": 1}

    def split(self, text):
        return [s.strip() for s in text.split(";") if s.strip()]

    def names(self):
        return ["Sin", "Cos"]

    def reset(self):
        self.calls.append(("reset",))
        return {"status": "ok"}

    def close(self):
        pass


class HttpRouting(unittest.TestCase):
    """Routing is covered by driving a real (loopback) server with a fake kernel."""

    @classmethod
    def setUpClass(cls):
        cls._saved = notebook._KERNEL
        notebook._KERNEL = FakeKernel()
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        cls.port = cls.server.server_address[1]
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        notebook._KERNEL = cls._saved

    def url(self, path):
        return "http://127.0.0.1:%d%s" % (self.port, path)

    def get(self, path):
        with urllib.request.urlopen(self.url(path), timeout=5) as r:
            return r.status, json.load(r)

    def post(self, path, body):
        req = urllib.request.Request(
            self.url(path), data=json.dumps(body).encode("utf-8"),
            headers={"Content-Type": "application/json"}, method="POST")
        with urllib.request.urlopen(req, timeout=5) as r:
            return r.status, json.load(r)

    def test_health(self):
        status, body = self.get("/api/health")
        self.assertEqual(status, 200)
        self.assertTrue(body["ok"])

    def test_names(self):
        _, body = self.get("/api/names")
        self.assertEqual(body["names"], ["Sin", "Cos"])

    def test_eval(self):
        _, body = self.post("/api/eval", {"input": "1+1"})
        self.assertEqual(body["result"], "42")

    def test_eval_renders_graphics_as_svg(self):
        _, body = self.post("/api/eval", {"input": "Plot[Sin[x], {x, 0, 1}]"})
        self.assertIn("svg", body)
        self.assertIn("<svg", body["svg"])

    def test_split(self):
        _, body = self.post("/api/split", {"input": "a; b; c"})
        self.assertEqual(body["statements"], ["a", "b", "c"])

    def test_reset(self):
        _, body = self.post("/api/reset", {})
        self.assertEqual(body["status"], "ok")

    def test_unknown_path_is_404(self):
        with self.assertRaises(urllib.error.HTTPError) as cm:
            self.get("/nope")
        self.assertEqual(cm.exception.code, 404)

    def test_malformed_json_does_not_crash_the_server(self):
        req = urllib.request.Request(
            self.url("/api/eval"), data=b"{not json",
            headers={"Content-Type": "application/json"}, method="POST")
        with urllib.request.urlopen(req, timeout=5) as r:
            body = json.load(r)
        # An unparseable body becomes {} and evaluates the empty input.
        self.assertIn("result", body)


if __name__ == "__main__":
    unittest.main()
