"""Unit tests for the MCP server: protocol dispatch, tools and both transports.

No real kernel is started: a fake stands in, so these run in the fast `make
py-test` set. The HTTP transport is driven over a real loopback socket, and the
stdio transport with in-memory text streams.
"""

import io
import json
import threading
import unittest
import urllib.error
import urllib.request
from http.server import ThreadingHTTPServer
from unittest import mock

from rmathpy import mcp


class FakeKernel:
    """A stand-in for the Rust kernel client (no subprocess)."""

    def __init__(self):
        self.calls = []

    def eval(self, text, steps=False):
        self.calls.append(("eval", text, steps))
        if text == "boom":
            return {"status": "error", "error": "Syntax error"}
        if text == "Plot[Sin[x], {x, 0, 1}]":
            return {"status": "ok", "result": "-Graphics-", "in": 1, "out": 1,
                    "graphics": {"type": "Graphics",
                                 "primitives": [{"type": "Line",
                                                 "pts": [[0, 0], [1, 1]]}],
                                 "plotrange": [0, 1, 0, 1]}}
        if text == "with-prints":
            return {"status": "ok", "result": "4", "in": 2, "out": 2,
                    "prints": ["hi"],
                    "messages": [{"tag": "General::spell", "text": "warning"}]}
        return {"status": "ok", "result": "42", "in": 1, "out": 1}

    def split(self, text):
        return [s.strip() for s in text.split(";") if s.strip()]

    def names(self):
        return ["Sin", "Cos"]

    def reset(self):
        self.calls.append(("reset",))
        return {"status": "ok"}

    def close(self):
        pass


class BrokenKernel(FakeKernel):
    """A kernel that cannot run — e.g. the binary is missing."""

    def eval(self, text, steps=False):
        raise OSError("no kernel binary")


def server_with_fake():
    fake = FakeKernel()
    return mcp.McpServer(kernel_factory=lambda: fake), fake


def call(server, msg_id, method, params=None):
    responses = server.handle({"jsonrpc": "2.0", "id": msg_id, "method": method,
                               "params": params or {}})
    assert len(responses) == 1, responses
    return responses[0]


class Lifecycle(unittest.TestCase):
    def test_initialize_echoes_a_supported_version(self):
        server, _ = server_with_fake()
        r = call(server, 1, "initialize",
                 {"protocolVersion": "2025-06-18", "clientInfo": {"name": "t"}})
        self.assertEqual(r["result"]["protocolVersion"], "2025-06-18")
        self.assertEqual(r["result"]["serverInfo"]["name"], "rmath")
        self.assertIn("tools", r["result"]["capabilities"])
        self.assertTrue(r["result"]["instructions"])

    def test_initialize_offers_the_preferred_version_when_unsupported(self):
        server, _ = server_with_fake()
        r = call(server, 1, "initialize", {"protocolVersion": "1999-01-01"})
        self.assertEqual(r["result"]["protocolVersion"], mcp.PROTOCOL_VERSION)

    def test_initialized_notification_has_no_response(self):
        server, _ = server_with_fake()
        self.assertEqual(server.handle({"jsonrpc": "2.0",
                                        "method": "notifications/initialized"}), [])
        self.assertTrue(server._initialized)

    def test_ping(self):
        server, _ = server_with_fake()
        self.assertEqual(call(server, 1, "ping")["result"], {})

    def test_unknown_method_is_an_error(self):
        server, _ = server_with_fake()
        r = call(server, 7, "does/not/exist")
        self.assertEqual(r["error"]["code"], -32601)
        self.assertEqual(r["id"], 7)

    def test_invalid_message(self):
        server, _ = server_with_fake()
        self.assertEqual(server.handle("nope")[0]["error"]["code"], -32600)


class Tools(unittest.TestCase):
    def test_list_has_core_tools(self):
        server, _ = server_with_fake()
        with mock.patch.object(mcp, "_lean_available", return_value=False):
            tools = call(server, 1, "tools/list")["result"]["tools"]
        names = {t["name"] for t in tools}
        self.assertLessEqual({"rmath_eval", "rmath_plot", "rmath_split",
                              "rmath_names", "rmath_reset", "rmath_help"}, names)
        self.assertNotIn("rmath_lean_verify", names)
        for t in tools:
            self.assertTrue(t["description"])
            self.assertEqual(t["inputSchema"]["type"], "object")

    def test_lean_tools_appear_when_available(self):
        server, _ = server_with_fake()
        with mock.patch.object(mcp, "_lean_available", return_value=True):
            names = {t["name"] for t in
                     call(server, 1, "tools/list")["result"]["tools"]}
        self.assertIn("rmath_lean_verify", names)
        self.assertIn("rmath_lean_check", names)

    def test_eval_returns_the_transcript(self):
        server, fake = server_with_fake()
        r = call(server, 1, "tools/call",
                 {"name": "rmath_eval", "arguments": {"input": "1 + 1"}})
        self.assertFalse(r["result"]["isError"])
        text = r["result"]["content"][0]["text"]
        self.assertIn("In[1]:= 1 + 1", text)
        self.assertIn("Out[1]= 42", text)
        self.assertEqual(fake.calls[-1], ("eval", "1 + 1", False))

    def test_eval_splits_statements(self):
        server, fake = server_with_fake()
        call(server, 1, "tools/call",
             {"name": "rmath_eval", "arguments": {"input": "a=1; b=2"}})
        self.assertEqual([c[1] for c in fake.calls], ["a=1", "b=2"])

    def test_eval_reports_prints_and_messages(self):
        server, _ = server_with_fake()
        r = call(server, 1, "tools/call",
                 {"name": "rmath_eval", "arguments": {"input": "with-prints"}})
        text = r["result"]["content"][0]["text"]
        self.assertIn("Print: hi", text)
        self.assertIn("General::spell: warning", text)

    def test_eval_graphics_include_svg(self):
        server, _ = server_with_fake()
        r = call(server, 1, "tools/call",
                 {"name": "rmath_eval",
                  "arguments": {"input": "Plot[Sin[x], {x, 0, 1}]"}})
        content = r["result"]["content"]
        self.assertEqual(len(content), 2)
        self.assertIn("In[1]:=", content[0]["text"])
        self.assertIn("<svg", content[1]["text"])

    def test_eval_error_is_a_tool_error(self):
        server, _ = server_with_fake()
        r = call(server, 1, "tools/call",
                 {"name": "rmath_eval", "arguments": {"input": "boom"}})
        self.assertTrue(r["result"]["isError"])
        self.assertIn("Syntax error", r["result"]["content"][0]["text"])

    def test_eval_requires_input(self):
        server, _ = server_with_fake()
        r = call(server, 1, "tools/call", {"name": "rmath_eval", "arguments": {}})
        self.assertEqual(r["error"]["code"], -32602)

    def test_plot_returns_svg(self):
        server, _ = server_with_fake()
        r = call(server, 1, "tools/call",
                 {"name": "rmath_plot",
                  "arguments": {"input": "Plot[Sin[x], {x, 0, 1}]"}})
        self.assertFalse(r["result"]["isError"])
        self.assertIn("<svg", r["result"]["content"][1]["text"])

    def test_plot_without_graphics_is_an_error(self):
        server, _ = server_with_fake()
        r = call(server, 1, "tools/call",
                 {"name": "rmath_plot", "arguments": {"input": "1 + 1"}})
        self.assertTrue(r["result"]["isError"])

    def test_split(self):
        server, _ = server_with_fake()
        r = call(server, 1, "tools/call",
                 {"name": "rmath_split", "arguments": {"input": "a; b"}})
        self.assertIn("1: a", r["result"]["content"][0]["text"])

    def test_names(self):
        server, _ = server_with_fake()
        r = call(server, 1, "tools/call", {"name": "rmath_names"})
        self.assertIn("Sin", r["result"]["content"][0]["text"])

    def test_reset(self):
        server, fake = server_with_fake()
        r = call(server, 1, "tools/call", {"name": "rmath_reset"})
        self.assertFalse(r["result"]["isError"])
        self.assertIn(("reset",), fake.calls)

    def test_help_is_static(self):
        server, _ = server_with_fake()
        r = call(server, 1, "tools/call", {"name": "rmath_help"})
        self.assertIn("rMath syntax", r["result"]["content"][0]["text"])

    def test_unknown_tool_is_invalid_params(self):
        server, _ = server_with_fake()
        r = call(server, 1, "tools/call",
                 {"name": "rmath_nope", "arguments": {}})
        self.assertEqual(r["error"]["code"], -32602)

    def test_lean_verify_is_not_callable_when_hidden(self):
        server, _ = server_with_fake()
        with mock.patch.object(mcp, "_lean_available", return_value=False):
            r = call(server, 1, "tools/call",
                     {"name": "rmath_lean_verify", "arguments": {"input": "x"}})
        self.assertEqual(r["error"]["code"], -32602)

    def test_tool_execution_failure_is_a_tool_error_not_a_protocol_error(self):
        # A missing/broken kernel is the tool failing, not a bad request, so the
        # call still returns a result with isError set.
        server = mcp.McpServer(kernel_factory=BrokenKernel)
        r = call(server, 1, "tools/call",
                 {"name": "rmath_eval", "arguments": {"input": "1 + 1"}})
        self.assertNotIn("error", r)
        self.assertTrue(r["result"]["isError"])
        self.assertIn("Tool execution failed", r["result"]["content"][0]["text"])


class StdioTransport(unittest.TestCase):
    def test_end_to_end_stream(self):
        server, _ = server_with_fake()
        lines = [
            {"jsonrpc": "2.0", "id": 1, "method": "initialize",
             "params": {"protocolVersion": "2025-11-25"}},
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/call",
             "params": {"name": "rmath_eval", "arguments": {"input": "1+1"}}},
        ]
        stdin = io.StringIO("".join(json.dumps(x) + "\n" for x in lines))
        stdout = io.StringIO()
        mcp.serve_stdio(server, stdin, stdout)
        out = [json.loads(l) for l in stdout.getvalue().splitlines()]
        self.assertEqual(len(out), 2)  # the notification produced nothing
        self.assertEqual(out[0]["id"], 1)
        self.assertEqual(out[1]["id"], 2)
        self.assertIn("Out[1]= 42", out[1]["result"]["content"][0]["text"])

    def test_malformed_line_gets_a_parse_error(self):
        server, _ = server_with_fake()
        stdout = io.StringIO()
        mcp.serve_stdio(server, io.StringIO("{not json\n"), stdout)
        out = json.loads(stdout.getvalue())
        self.assertEqual(out["error"]["code"], -32700)


class HttpTransport(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server, cls.fake = server_with_fake()
        cls.httpd = ThreadingHTTPServer(
            ("127.0.0.1", 0), mcp._http_handler_class(cls.server))
        cls.port = cls.httpd.server_address[1]
        cls.thread = threading.Thread(target=cls.httpd.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.httpd.shutdown()
        cls.httpd.server_close()
        cls.server.close()

    def url(self, path):
        return "http://127.0.0.1:%d%s" % (self.port, path)

    def post(self, payload, headers=None):
        data = json.dumps(payload).encode("utf-8")
        req = urllib.request.Request(self.url("/mcp"), data=data, method="POST",
                                     headers={"Content-Type": "application/json",
                                              **(headers or {})})
        return urllib.request.urlopen(req, timeout=5)

    def test_initialize_over_http(self):
        with self.post({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                        "params": {"protocolVersion": "2025-11-25"}}) as r:
            self.assertEqual(r.status, 200)
            body = json.load(r)
        self.assertEqual(body["result"]["serverInfo"]["name"], "rmath")

    def test_notification_is_202_without_body(self):
        with self.post({"jsonrpc": "2.0",
                        "method": "notifications/initialized"}) as r:
            self.assertEqual(r.status, 202)
            self.assertEqual(r.read(), b"")

    def test_empty_body_is_a_bad_request(self):
        # An empty POST body is not a JSON-RPC notification.
        req = urllib.request.Request(self.url("/mcp"), data=b"", method="POST",
                                     headers={"Content-Type": "application/json"})
        with self.assertRaises(urllib.error.HTTPError) as cm:
            urllib.request.urlopen(req, timeout=5)
        self.assertEqual(cm.exception.code, 400)

    def test_cross_site_origin_is_refused(self):
        with self.assertRaises(urllib.error.HTTPError) as cm:
            self.post({"jsonrpc": "2.0", "id": 1, "method": "ping"},
                      headers={"Origin": "http://evil.example"})
        self.assertEqual(cm.exception.code, 403)

    def test_foreign_host_is_refused(self):
        with self.assertRaises(urllib.error.HTTPError) as cm:
            self.post({"jsonrpc": "2.0", "id": 1, "method": "ping"},
                      headers={"Host": "evil.example:%d" % self.port})
        self.assertEqual(cm.exception.code, 403)

    def test_get_mcp_is_405(self):
        with self.assertRaises(urllib.error.HTTPError) as cm:
            urllib.request.urlopen(self.url("/mcp"), timeout=5)
        self.assertEqual(cm.exception.code, 405)

    def test_health(self):
        with urllib.request.urlopen(self.url("/health"), timeout=5) as r:
            self.assertEqual(r.status, 200)
            body = json.load(r)
        self.assertTrue(body["ok"])

    def test_unknown_path_is_404(self):
        with self.assertRaises(urllib.error.HTTPError) as cm:
            urllib.request.urlopen(self.url("/nope"), timeout=5)
        self.assertEqual(cm.exception.code, 404)


if __name__ == "__main__":
    unittest.main()
