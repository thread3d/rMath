"""Unit tests for the notebook front end's pure helpers and HTTP routing."""

import json
import os
import shutil
import tempfile
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


class ResolveInWorkdir(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="rmath-resolve-")
        self.addCleanup(shutil.rmtree, self.tmp, True)
        patcher = mock.patch.dict(os.environ, {"RMATH_WORKDIR": self.tmp})
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_relative_name_resolves_inside(self):
        got = notebook.resolve_in_workdir("notes.mt")
        self.assertEqual(got, os.path.join(os.path.realpath(self.tmp), "notes.mt"))

    def test_dotdot_and_absolute_paths_are_refused(self):
        self.assertIsNone(notebook.resolve_in_workdir("../escape.mt"))
        self.assertIsNone(notebook.resolve_in_workdir("/etc/hostname"))
        self.assertIsNone(notebook.resolve_in_workdir("sub/../../escape.mt"))

    def test_empty_names_are_refused(self):
        self.assertIsNone(notebook.resolve_in_workdir(""))
        self.assertIsNone(notebook.resolve_in_workdir(None))

    def test_absolute_path_inside_is_allowed(self):
        inside = os.path.join(self.tmp, "a.mt")
        self.assertEqual(notebook.resolve_in_workdir(inside), inside)


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

    def post(self, path, body, extra_headers=None):
        headers = {"Content-Type": "application/json"}
        if extra_headers:
            headers.update(extra_headers)
        req = urllib.request.Request(
            self.url(path), data=json.dumps(body).encode("utf-8"),
            headers=headers, method="POST")
        with urllib.request.urlopen(req, timeout=5) as r:
            return r.status, json.load(r)

    def test_health(self):
        status, body = self.get("/api/health")
        self.assertEqual(status, 200)
        self.assertTrue(body["ok"])

    def test_api_index_lists_the_routes_and_mcp(self):
        status, body = self.get("/api")
        self.assertEqual(status, 200)
        self.assertEqual(body["name"], "rMath notebook API")
        self.assertEqual(body["mcp"]["command"], ["python3", "-m", "rmathpy.mcp"])
        paths = {e["path"] for e in body["endpoints"]}
        self.assertIn("/api/eval", paths)
        self.assertIn("/api/lean/verify", paths)
        self.assertIn("/api", paths)
        for endpoint in body["endpoints"]:
            self.assertIn(endpoint["method"], ("GET", "POST"))

    def test_openapi_document_covers_eval(self):
        status, body = self.get("/api/openapi.json")
        self.assertEqual(status, 200)
        self.assertEqual(body["openapi"], "3.1.0")
        self.assertIn("/api/eval", body["paths"])
        self.assertIn("post", body["paths"]["/api/eval"])
        schema = body["paths"]["/api/eval"]["post"]["requestBody"]["content"][
            "application/json"]["schema"]
        self.assertIn("input", schema["properties"])

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

    def test_index_page_is_served(self):
        with urllib.request.urlopen(self.url("/"), timeout=5) as r:
            status, body = r.status, r.read().decode("utf-8")
        self.assertEqual(status, 200)
        self.assertIn("<!DOCTYPE html>", body)
        self.assertIn("</html>", body)

    def test_malformed_json_does_not_crash_the_server(self):
        req = urllib.request.Request(
            self.url("/api/eval"), data=b"{not json",
            headers={"Content-Type": "application/json"}, method="POST")
        with urllib.request.urlopen(req, timeout=5) as r:
            body = json.load(r)
        # An unparseable body becomes {} and evaluates the empty input.
        self.assertIn("result", body)

    # ---- the file endpoints stay inside the working directory -------------
    def _workdir(self):
        tmp = tempfile.mkdtemp(prefix="rmath-wd-")
        self.addCleanup(shutil.rmtree, tmp, True)
        patcher = mock.patch.dict(os.environ, {"RMATH_WORKDIR": tmp})
        patcher.start()
        self.addCleanup(patcher.stop)
        return tmp

    def test_save_writes_inside_the_workdir(self):
        tmp = self._workdir()
        _, body = self.post("/api/save", {"inputs": ["1+1"], "name": "notes.mt"})
        self.assertEqual(body["status"], "ok")
        with open(os.path.join(tmp, "notes.mt")) as f:
            self.assertIn("1+1", f.read())

    def test_save_default_name_lands_in_the_workdir(self):
        tmp = self._workdir()
        _, body = self.post("/api/save", {"inputs": ["2+2"]})
        self.assertEqual(body["status"], "ok")
        self.assertEqual(os.path.dirname(body["path"]), os.path.realpath(tmp))

    def test_save_refuses_to_escape_the_workdir(self):
        tmp = self._workdir()
        _, body = self.post("/api/save",
                            {"inputs": ["1+1"], "name": "../escape.mt"})
        self.assertEqual(body["status"], "error")
        self.assertIn("working directory", body["error"])
        self.assertFalse(os.path.exists(os.path.join(os.path.dirname(tmp),
                                                    "escape.mt")))

    def test_save_refuses_an_absolute_path(self):
        self._workdir()
        _, body = self.post("/api/save",
                            {"inputs": ["1+1"],
                             "name": os.path.join(tempfile.gettempdir(),
                                                 "rmath-absolute.mt")})
        self.assertEqual(body["status"], "error")
        self.assertIn("working directory", body["error"])

    def test_save_svg_refuses_to_escape_the_workdir(self):
        tmp = self._workdir()
        _, body = self.post("/api/save-svg", {"svg": "<svg/>",
                                              "name": "../../plot.svg"})
        self.assertEqual(body["status"], "error")
        self.assertFalse(os.path.exists(os.path.join(os.path.dirname(tmp),
                                                    "plot.svg")))

    def test_open_reads_a_script_relative_to_the_workdir(self):
        tmp = self._workdir()
        with open(os.path.join(tmp, "demo.mt"), "w") as f:
            f.write("a; b\n")
        _, body = self.post("/api/open", {"path": "demo.mt"})
        self.assertEqual(body["status"], "ok")
        self.assertEqual(body["statements"], ["a", "b"])

    def test_open_accepts_an_absolute_path_inside_the_workdir(self):
        tmp = self._workdir()
        path = os.path.join(tmp, "sub.mt")
        with open(path, "w") as f:
            f.write("c\n")
        _, body = self.post("/api/open", {"path": path})
        self.assertEqual(body["status"], "ok")

    def test_open_refuses_a_path_outside_the_workdir(self):
        self._workdir()
        _, body = self.post("/api/open", {"path": "/etc/hostname"})
        self.assertEqual(body["status"], "error")
        self.assertIn("working directory", body["error"])
        _, body = self.post("/api/open", {"path": "../outside.mt"})
        self.assertEqual(body["status"], "error")

    # ---- cross-site / DNS-rebinding guard ---------------------------------
    def test_same_origin_post_is_allowed(self):
        _, body = self.post("/api/eval", {"input": "1+1"},
                            {"Origin": "http://127.0.0.1:%d" % self.port})
        self.assertEqual(body["result"], "42")

    def test_localhost_origin_on_the_same_port_is_allowed(self):
        _, body = self.post("/api/eval", {"input": "1+1"},
                            {"Origin": "http://localhost:%d" % self.port})
        self.assertEqual(body["result"], "42")

    def test_no_origin_is_allowed_for_non_browser_clients(self):
        # This is what `curl -X POST /api/eval` does.
        _, body = self.post("/api/eval", {"input": "1+1"})
        self.assertEqual(body["result"], "42")

    def test_cross_site_origin_is_refused(self):
        with self.assertRaises(urllib.error.HTTPError) as cm:
            self.post("/api/eval", {"input": "1+1"}, {"Origin": "http://evil.example"})
        self.assertEqual(cm.exception.code, 403)

    def test_opaque_origin_is_refused(self):
        with self.assertRaises(urllib.error.HTTPError) as cm:
            self.post("/api/eval", {"input": "1+1"}, {"Origin": "null"})
        self.assertEqual(cm.exception.code, 403)

    def test_origin_on_another_port_is_refused(self):
        with self.assertRaises(urllib.error.HTTPError) as cm:
            self.post("/api/eval", {"input": "1+1"},
                      {"Origin": "http://127.0.0.1:%d" % (self.port + 1)})
        self.assertEqual(cm.exception.code, 403)

    def test_sec_fetch_cross_site_is_refused(self):
        with self.assertRaises(urllib.error.HTTPError) as cm:
            self.post("/api/eval", {"input": "1+1"}, {"Sec-Fetch-Site": "cross-site"})
        self.assertEqual(cm.exception.code, 403)

    def test_foreign_host_is_refused(self):
        with self.assertRaises(urllib.error.HTTPError) as cm:
            self.post("/api/eval", {"input": "1+1"}, {"Host": "evil.example"})
        self.assertEqual(cm.exception.code, 403)

    def test_get_with_foreign_host_is_refused(self):
        req = urllib.request.Request(self.url("/api/health"),
                                     headers={"Host": "evil.example"})
        with self.assertRaises(urllib.error.HTTPError) as cm:
            urllib.request.urlopen(req, timeout=5)
        self.assertEqual(cm.exception.code, 403)


class SplitHost(unittest.TestCase):
    def test_plain_host(self):
        self.assertEqual(notebook._split_host("127.0.0.1:8787"), ("127.0.0.1", "8787"))
        self.assertEqual(notebook._split_host("localhost"), ("localhost", ""))

    def test_bracketed_ipv6(self):
        self.assertEqual(notebook._split_host("[::1]:8787"), ("::1", "8787"))
        self.assertEqual(notebook._split_host("[::1]"), ("::1", ""))

    def test_empty(self):
        self.assertEqual(notebook._split_host(""), ("", ""))


class IndexPage(unittest.TestCase):
    def test_index_html_reads_the_static_file(self):
        # The page lives in rmathpy/static/notebook.html, not a string literal.
        html = notebook.index_html()
        self.assertIn("<!DOCTYPE html>", html)
        self.assertIn("</html>", html)
        self.assertIs(notebook.index_html(), html)  # cached, not re-read


class ApiSpec(unittest.TestCase):
    """The discovery index and the OpenAPI document come from one table."""

    def test_index_and_openapi_describe_the_same_routes(self):
        from rmathpy import apispec
        index = apispec.index(9000)
        spec = apispec.openapi(9000)
        self.assertEqual(index["base_url"], "http://127.0.0.1:9000")
        # Every spec'd API endpoint appears in both documents with its method.
        for (_m, path, _s, _q, _r, _t, _st) in apispec.ENDPOINTS:
            self.assertIn(path, spec["paths"], path)
            self.assertIn(_m.lower(), spec["paths"][path])
        indexed = {e["path"] for e in index["endpoints"]}
        self.assertEqual(indexed, {p for (_m, p, _s, _q, _r, _t, _st) in apispec.ENDPOINTS}
                         | {"/", "/katex/{path}"})

    def test_streaming_endpoint_declares_ndjson(self):
        from rmathpy import apispec
        spec = apispec.openapi()
        responses = spec["paths"]["/api/agent/stream"]["post"]["responses"]
        self.assertIn("application/x-ndjson", responses["200"]["content"])

    def test_endpoint_requests_are_json_schemas(self):
        from rmathpy import apispec
        for (_m, path, _s, request, _r, _t, _st) in apispec.ENDPOINTS:
            if request is None:
                continue
            self.assertEqual(request["type"], "object", path)


if __name__ == "__main__":
    unittest.main()
