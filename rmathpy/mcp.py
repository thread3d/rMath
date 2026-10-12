"""Model Context Protocol (MCP) server for the rMath symbolic kernel.

Two standard transports, both standard-library only:

* **stdio** (default) — newline-delimited JSON-RPC on stdin/stdout, which is how
  local MCP clients launch a server::

      python3 -m rmathpy.mcp

* **Streamable HTTP** — a single ``POST /mcp`` endpoint (JSON response mode),
  bound to loopback by default::

      python3 -m rmathpy.mcp --http --port 8790

The protocol logic lives in :class:`McpServer` and is transport-independent, so
tests drive it directly without spawning anything. The kernel itself is the same
``Kernel`` client the notebook uses, so a tool call and a notebook cell share
one implementation and one persistent session per process.
"""

import argparse
import json
import os
import signal
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse

# Protocol revisions this server can speak, newest first. The client's requested
# version is echoed when supported; otherwise we answer with our preferred one,
# which is the negotiation the MCP lifecycle prescribes.
PROTOCOL_VERSION = "2025-11-25"
SUPPORTED_VERSIONS = ("2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05")

SERVER_NAME = "rmath"
SERVER_TITLE = "rMath symbolic kernel"
SERVER_VERSION = "1.0.0"

INSTRUCTIONS = (
    "rMath is a self-contained Mathematica-style symbolic kernel (exact "
    "arithmetic, algebra, calculus, linear algebra, plotting). It keeps "
    "definitions between calls: evaluate in order and later calls can reuse "
    "earlier ones.\n\n"
    "Syntax: calls use square brackets (Sin[x], D[f, x], Integrate[f, x], "
    "Solve[eq, x], Expand[p], Factor[p]); lists are {a, b, c}; rules are "
    "x -> 2 and replacement is expr /. rule; definitions are f[x_] := x^2 and "
    "f[0] = 1; N[Pi, 50] gives arbitrary precision; "
    "Plot[Sin[x], {x, 0, 2 Pi}] renders graphics.\n\n"
    "Use rmath_eval to compute, rmath_plot to get a plot as SVG, rmath_names to "
    "see what is defined, and rmath_reset to clear user definitions. When the "
    "Lean bridge is available, rmath_lean_verify can prove an rMath claim in "
    "Mathlib."
)


def _lean_available() -> bool:
    """Whether the Lean bridge can be offered. Indirection for tests."""
    try:
        from . import lean
        return lean.available()
    except Exception:
        return False


def _text(text):
    return {"type": "text", "text": text}


def _ok(items):
    return {"content": items if isinstance(items, list) else [_text(str(items))],
            "isError": False}


def _fail(message):
    return {"content": [_text(message)], "isError": True}


def _schema(props, required=()):
    schema = {"type": "object", "properties": props, "additionalProperties": False}
    if required:
        schema["required"] = list(required)
    return schema


_STRING_DESC = "Mathematica-style rMath source; several statements separated by newlines or ';'."

# The always-present tools. Lean tools are appended when the toolchain is found.
_TOOLS = [
    {
        "name": "rmath_eval",
        "title": "Evaluate rMath",
        "description": (
            "Evaluate one or more rMath (Mathematica-style) statements in the "
            "persistent kernel session and return the In/Out transcript. "
            "Definitions, assignments and loaded files persist for later calls. "
            "A graphics result is also returned as an SVG text block."),
        "inputSchema": _schema({
            "input": {"type": "string", "description": _STRING_DESC},
            "steps": {"type": "boolean", "default": False,
                      "description": "Include the kernel's step-by-step trace."},
        }, required=["input"]),
    },
    {
        "name": "rmath_plot",
        "title": "Plot to SVG",
        "description": ("Evaluate an expression that produces graphics and return "
                        "the rendered SVG. Example: Plot[Sin[x], {x, 0, 2 Pi}]."),
        "inputSchema": _schema({
            "input": {"type": "string", "description": "A graphics-producing expression."},
            "width": {"type": "integer", "default": 360},
            "height": {"type": "integer", "default": 240},
        }, required=["input"]),
    },
    {
        "name": "rmath_split",
        "title": "Split statements",
        "description": "Split a script into top-level statements with the kernel's own splitter.",
        "inputSchema": _schema({
            "input": {"type": "string", "description": _STRING_DESC},
        }, required=["input"]),
    },
    {
        "name": "rmath_names",
        "title": "List symbols",
        "description": "List the names of the symbols the kernel currently knows about.",
        "inputSchema": _schema({}),
    },
    {
        "name": "rmath_reset",
        "title": "Reset the kernel",
        "description": "Clear every user definition, restoring a pristine kernel.",
        "inputSchema": _schema({}),
    },
    {
        "name": "rmath_help",
        "title": "rMath syntax help",
        "description": "A short cheat sheet of rMath syntax and example expressions.",
        "inputSchema": _schema({}),
    },
]

_LEAN_TOOLS = [
    {
        "name": "rmath_lean_check",
        "title": "Check Lean",
        "description": ("Type-check a Lean 4 snippet against Mathlib and return "
                        "the diagnostics."),
        "inputSchema": _schema({
            "code": {"type": "string", "description": "Lean 4 source (Mathlib is imported automatically)."},
        }, required=["code"]),
    },
    {
        "name": "rmath_lean_verify",
        "title": "Verify a claim in Lean",
        "description": ("Translate an rMath claim (expression and its claimed result) "
                        "to a Lean statement and try to prove it with Mathlib. "
                        "Returns an outcome of verified, unproved, cannot verify or error."),
        "inputSchema": _schema({
            "input": {"type": "string", "description": "The left-hand rMath expression, e.g. D[Sin[x]^2, x]."},
            "result": {"type": "string", "description": "The claimed right-hand side, e.g. 2 Cos[x] Sin[x]."},
            "relation": {"type": "string", "enum": ["=", "<", "<=", ">", ">=", "!="],
                         "default": "="},
        }, required=["input"]),
    },
]

_HELP = """rMath syntax cheat sheet
========================

Calls use square brackets; arguments are comma-separated:
  Sin[x]   Sqrt[x]   Log[x]   Exp[x]   Abs[x]   Max[a, b]   Mod[m, n]

Exact arithmetic and algebra:
  1/2 + 1/3                 -> 5/6
  Expand[(x + 1)^3]         -> 1 + 3 x + 3 x^2 + x^3
  Factor[x^2 - 1]           -> (-1 + x) (1 + x)
  Together[1/x + 1/y]       -> (x + y)/(x y)
  Simplify[Sin[x]^2 + Cos[x]^2] -> 1

Calculus:
  D[Sin[x]^2, x]            -> 2 Cos[x] Sin[x]
  Integrate[x^2, x]         -> x^3/3
  Integrate[x^2, {x, 0, 1}] -> 1/3
  Limit[Sin[x]/x, x -> 0]   -> 1
  Sum[1/n^2, {n, 1, Infinity}] -> Pi^2/6

Equations and linear algebra:
  Solve[x^2 == 2, x]        -> {{x -> -Sqrt[2]}, {x -> Sqrt[2]}}
  Det[{{1, 2}, {3, 4}}]     -> -2
  Inverse[{{1, 2}, {3, 4}}]

Lists and functional programming:
  Table[n^2, {n, 1, 5}]     -> {1, 4, 9, 16, 25}
  Map[Sqrt, {1, 4, 9}]      -> {1, 2, 3}
  Fold[Plus, 0, {1, 2, 3}]  -> 6
  Sort[{3, 1, 2}]           -> {1, 2, 3}

Patterns, rules and definitions:
  f[x_] := x^2              define a function
  f[0] = 1                  a specific value
  {a, b} /. a -> 1          replace
  Cases[{1, 2, 3}, _?EvenQ] -> {2}

Numerics, constants and output:
  N[Pi, 50]                 50-digit Pi
  Pi, E, I, Infinity
  Plot[Sin[x], {x, 0, 2 Pi}]  graphics (SVG via rmath_plot)
  Print["label ", expr]      text output
  Get["demo/demo.mt"]        load the demo tour

`?name` prints information about a symbol; `Names[]` lists names.
"""


class McpServer:
    """Transport-independent MCP request handler.

    ``kernel_factory`` lets a test inject a stand-in; the real default lazily
    starts ``rmath --server`` on first use.
    """

    def __init__(self, kernel_factory=None):
        self._kernel_factory = kernel_factory
        self._kernel = None
        self._lock = threading.RLock()
        self._initialized = False
        self._client = {}
        self._protocol = PROTOCOL_VERSION

    # ---- lifecycle -------------------------------------------------------

    def kernel(self):
        with self._lock:
            if self._kernel is None:
                factory = self._kernel_factory
                if factory is None:
                    from .kernel import Kernel
                    factory = Kernel
                self._kernel = factory()
            return self._kernel

    def close(self):
        with self._lock:
            if self._kernel is not None:
                try:
                    self._kernel.close()
                except Exception:
                    pass
                self._kernel = None

    def tools(self) -> list:
        tools = list(_TOOLS)
        if _lean_available():
            tools.extend(_LEAN_TOOLS)
        return tools

    # ---- JSON-RPC dispatch ----------------------------------------------

    def handle(self, message):
        """Handle one decoded JSON-RPC message; return a list of responses.

        Notifications produce no response, so the list is empty for them. A
        batch (an array) is answered with a list of responses, which keeps a
        tolerant client working even though the current MCP revision does not
        use batching.
        """
        if isinstance(message, list):
            out = []
            for item in message:
                out.extend(self.handle(item))
            return out
        if not isinstance(message, dict):
            return [self._error(None, -32600, "Invalid Request")]

        method = message.get("method")
        msg_id = message.get("id")
        is_notification = "id" not in message
        if not isinstance(method, str):
            if is_notification:
                return []
            return [self._error(msg_id, -32600, "Invalid Request: missing method")]

        if is_notification:
            self._note_notification(method, message.get("params") or {})
            return []

        try:
            if method == "initialize":
                return [self._response(msg_id, self._initialize(message.get("params") or {}))]
            if method == "ping":
                return [self._response(msg_id, {})]
            if method == "tools/list":
                return [self._response(msg_id, {"tools": self.tools()})]
            if method == "tools/call":
                return [self._response(msg_id, self._tools_call_safe(message.get("params") or {}))]
            if method == "resources/list":
                return [self._response(msg_id, {"resources": []})]
            if method == "prompts/list":
                return [self._response(msg_id, {"prompts": []})]
            if method == "logging/setLevel":
                return [self._response(msg_id, {})]
            return [self._error(msg_id, -32601, "Method not found: %s" % method)]
        except _RpcError as e:
            return [self._error(msg_id, e.code, e.message, e.data)]
        except Exception as e:  # never let a tool bug kill the server
            return [self._error(msg_id, -32603, "Internal error: %s" % e)]

    def _note_notification(self, method, params):
        if method == "notifications/initialized":
            self._initialized = True
        # notifications/cancelled and roots/list_changed need no action: tools
        # run to completion synchronously and no roots are consulted.

    def _initialize(self, params):
        requested = params.get("protocolVersion")
        self._protocol = requested if requested in SUPPORTED_VERSIONS else PROTOCOL_VERSION
        self._client = params.get("clientInfo") or {}
        return {
            "protocolVersion": self._protocol,
            "capabilities": {"tools": {"listChanged": False}},
            "serverInfo": {
                "name": SERVER_NAME,
                "title": SERVER_TITLE,
                "version": SERVER_VERSION,
            },
            "instructions": INSTRUCTIONS,
        }

    def _tools_call(self, params):
        name = params.get("name")
        arguments = params.get("arguments") or {}
        if not isinstance(name, str) or not name:
            raise _RpcError(-32602, "Invalid params: 'name' is required")
        if not isinstance(arguments, dict):
            raise _RpcError(-32602, "Invalid params: 'arguments' must be an object")
        known = {t["name"] for t in self.tools()}
        if name not in known:
            raise _RpcError(-32602, "Unknown tool: %s" % name)
        handler = getattr(self, "_tool_" + name[6:], None)  # strip "rmath_"
        if handler is None:
            raise _RpcError(-32601, "Tool not implemented: %s" % name)
        return handler(arguments)

    def _tools_call_safe(self, params):
        """Run a tool, reporting *execution* failures as a tool result.

        MCP distinguishes protocol errors (bad tool name or arguments, which
        stay JSON-RPC errors) from a tool that ran and failed — the latter is a
        normal result with ``isError: true``. A missing kernel binary, a crashed
        subprocess or a renderer bug is the second kind, and this keeps the
        agent connected instead of turning it into an opaque internal error.
        """
        try:
            return self._tools_call(params)
        except _RpcError:
            raise
        except Exception as e:
            return _fail("Tool execution failed: %s" % e)

    @staticmethod
    def _response(msg_id, result):
        return {"jsonrpc": "2.0", "id": msg_id, "result": result}

    @staticmethod
    def _error(msg_id, code, message, data=None):
        err = {"code": code, "message": message}
        if data is not None:
            err["data"] = data
        return {"jsonrpc": "2.0", "id": msg_id, "error": err}

    # ---- tool implementations -------------------------------------------

    def _arg(self, args, key, required=True):
        value = args.get(key)
        if value is None:
            if required:
                raise _RpcError(-32602, "Invalid params: '%s' is required" % key)
            return None
        if not isinstance(value, str):
            raise _RpcError(-32602, "Invalid params: '%s' must be a string" % key)
        return value

    def _tool_eval(self, args):
        text = self._arg(args, "input")
        if not text.strip():
            return _fail("The 'input' argument is empty.")
        steps = bool(args.get("steps"))
        with self._lock:
            k = self.kernel()
            statements = k.split(text) or [text]
            transcript = []
            svg = None
            for stmt in statements:
                r = k.eval(stmt, steps=steps)
                if r.get("status") != "ok":
                    transcript.append("In:= %s" % stmt)
                    transcript.append("Error: %s" % r.get("error", "evaluation failed"))
                    return _fail("\n".join(transcript))
                transcript.extend(self._format_response(stmt, r))
                if r.get("graphics") and svg is None:
                    svg = self._render(r["graphics"])
        items = [_text("\n".join(transcript) or "(no output)")]
        if svg:
            items.append(_text(svg))
        return _ok(items)

    def _format_response(self, stmt, r):
        lines = []
        inp = r.get("in")
        out = r.get("out")
        lines.append("In[%s]:= %s" % (inp, stmt) if inp is not None else "In:= %s" % stmt)
        for p in r.get("prints") or []:
            lines.append("Print: %s" % p)
        for m in r.get("messages") or []:
            tag = m.get("tag") or "Message"
            lines.append("%s: %s" % (tag, m.get("text", "")))
        result = r.get("result")
        if result not in (None, "Null"):
            lines.append("Out[%s]= %s" % (out, result) if out is not None
                         else "Out= %s" % result)
        return lines

    def _tool_plot(self, args):
        text = self._arg(args, "input")
        if not text.strip():
            return _fail("The 'input' argument is empty.")
        width = self._int_arg(args, "width", 360, 40, 4000)
        height = self._int_arg(args, "height", 240, 40, 4000)
        if width is False or height is False:
            return _fail("'width' and 'height' must be integers between 40 and 4000.")
        with self._lock:
            k = self.kernel()
            r = k.eval(text)
        if r.get("status") != "ok":
            return _fail("Error: %s" % r.get("error", "evaluation failed"))
        if not r.get("graphics"):
            shown = r.get("result")
            return _fail("The expression produced no graphics%s."
                         % (" (result was %s)" % shown if shown else ""))
        svg = self._render(r["graphics"], width=width, height=height)
        if svg is None:
            return _fail("The kernel produced graphics that could not be rendered.")
        return _ok([_text("SVG plot:"), _text(svg)])

    def _tool_split(self, args):
        text = self._arg(args, "input")
        with self._lock:
            statements = self.kernel().split(text)
        if not statements:
            return _ok(["No statements."])
        return _ok([_text("\n".join("%d: %s" % (i + 1, s)
                                    for i, s in enumerate(statements)))])

    def _tool_names(self, args):
        with self._lock:
            names = self.kernel().names()
        if not names:
            return _ok(["No names."])
        return _ok([_text("%d names:\n%s" % (len(names), "\n".join(names)))])

    def _tool_reset(self, args):
        with self._lock:
            r = self.kernel().reset()
        if r.get("status") != "ok":
            return _fail("Reset failed: %s" % r.get("error", "unknown error"))
        return _ok(["Kernel reset: all user definitions were cleared."])

    def _tool_help(self, args):
        return _ok([_text(_HELP)])

    def _tool_lean_check(self, args):
        code = self._arg(args, "code")
        from . import lean
        r = lean.run_lean(code)
        lines = []
        if r.get("error"):
            lines.append("Error: %s" % r["error"])
        for d in r.get("diagnostics") or []:
            lines.append("%s (%s:%s): %s" % (d.get("severity", "info"),
                                             d.get("line", "?"), d.get("col", "?"),
                                             d.get("message", "")))
        if not lines:
            lines.append("OK: no diagnostics.")
        return _ok([_text("\n".join(lines))])

    def _tool_lean_verify(self, args):
        inp = self._arg(args, "input")
        result = self._arg(args, "result", required=False)
        relation = args.get("relation") or "="
        if relation not in ("=", "<", "<=", ">", ">=", "!="):
            raise _RpcError(-32602, "Invalid params: bad 'relation'")
        from . import lean
        d = lean.verify(inp, result, relation=relation)
        lines = ["Outcome: %s" % d.get("outcome", "unknown")]
        statement = d.get("lean_display") or d.get("proposition") or d.get("lean")
        if statement:
            lines.append("Lean: %s" % statement)
        for diag in d.get("errors") or d.get("diagnostics") or []:
            lines.append("%s: %s" % (diag.get("severity", "error"),
                                     diag.get("message", "")))
        if d.get("error"):
            lines.append("Detail: %s" % d["error"])
        return _ok([_text("\n".join(lines))])

    # ---- helpers ---------------------------------------------------------

    def _int_arg(self, args, key, default, lo, hi):
        value = args.get(key, default)
        if isinstance(value, bool) or not isinstance(value, int):
            return False
        return value if lo <= value <= hi else False

    @staticmethod
    def _render(graphics, width=360, height=240):
        try:
            from .svg import render_graphics
            return render_graphics(graphics, width=width, height=height)
        except Exception:
            return None


class _RpcError(Exception):
    def __init__(self, code, message, data=None):
        super().__init__(message)
        self.code = code
        self.message = message
        self.data = data


# --------------------------------------------------------------------------
# stdio transport
# --------------------------------------------------------------------------

def _write_message(stream, obj):
    stream.write(json.dumps(obj, ensure_ascii=False) + "\n")
    stream.flush()


def serve_stdio(server, stdin=None, stdout=None):
    """Read newline-delimited JSON-RPC from ``stdin``, answer on ``stdout``."""
    stdin = stdin if stdin is not None else sys.stdin
    stdout = stdout if stdout is not None else sys.stdout
    for line in stdin:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line)
        except ValueError as e:
            _write_message(stdout, {"jsonrpc": "2.0", "id": None,
                                    "error": {"code": -32700,
                                              "message": "Parse error: %s" % e}})
            continue
        for response in server.handle(message):
            _write_message(stdout, response)
    return 0


# --------------------------------------------------------------------------
# Streamable HTTP transport (JSON response mode)
# --------------------------------------------------------------------------

def _origin_allowed(handler, port) -> bool:
    """The same DNS-rebinding/origin check the notebook applies."""
    from .notebook import _LOCAL_HOSTNAMES, _split_host
    host, host_port = _split_host(handler.headers.get("Host"))
    if host and host not in _LOCAL_HOSTNAMES:
        return False
    if host_port and host_port != str(port):
        return False
    if (handler.headers.get("Sec-Fetch-Site") or "").strip().lower() == "cross-site":
        return False
    origin = (handler.headers.get("Origin") or "").strip()
    if origin:
        parsed = urlparse(origin)
        if parsed.scheme not in ("http", "https"):
            return False
        if (parsed.hostname or "").lower() not in _LOCAL_HOSTNAMES:
            return False
        if parsed.port and str(parsed.port) != str(port):
            return False
    return True


def _http_handler_class(server):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, fmt, *args):
            if os.environ.get("RMATH_LOG_REQUESTS") == "1":
                sys.stderr.write("mcp %s\n" % (fmt % args))
                sys.stderr.flush()

        def _send(self, code, ctype, payload: bytes):
            self.send_response(code)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(payload)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            if payload:
                self.wfile.write(payload)

        def _json(self, code, obj):
            self._send(code, "application/json", json.dumps(obj).encode("utf-8"))

        def _deny_origin(self):
            self._json(403, {"jsonrpc": "2.0", "id": None,
                             "error": {"code": -32600,
                                       "message": "cross-site request refused"}})

        def do_POST(self):
            if urlparse(self.path).path != "/mcp":
                self._send(404, "text/plain", b"not found")
                return
            if not _origin_allowed(self, self.server.server_address[1]):
                self._deny_origin()
                return
            length = int(self.headers.get("Content-Length", 0) or 0)
            raw = self.rfile.read(length) if length else b""
            if not raw.strip():
                # A POST body must be one JSON-RPC message; an empty one is not
                # a notification, so it is a bad request (MCP Streamable HTTP).
                self._json(400, {"jsonrpc": "2.0", "id": None,
                                 "error": {"code": -32700,
                                           "message": "Parse error: empty body"}})
                return
            try:
                message = json.loads(raw.decode("utf-8"))
            except (ValueError, UnicodeDecodeError) as e:
                self._json(400, {"jsonrpc": "2.0", "id": None,
                                 "error": {"code": -32700,
                                           "message": "Parse error: %s" % e}})
                return
            responses = server.handle(message)
            if not responses:
                # A notification (or a response): accepted, no body.
                self._send(202, "application/json", b"")
                return
            payload = responses[0] if len(responses) == 1 else responses
            self._json(200, payload)

        def do_GET(self):
            path = urlparse(self.path).path
            if path == "/health":
                self._json(200, {"ok": True, "server": SERVER_NAME,
                                 "version": SERVER_VERSION})
            elif path == "/mcp":
                # We only offer the JSON response mode; no server-initiated SSE.
                self._send(405, "text/plain", b"GET not supported; POST JSON-RPC to /mcp")
            else:
                self._send(404, "text/plain", b"not found")

        def do_DELETE(self):
            self._send(405, "text/plain", b"session management is not supported")

    return Handler


def serve_http(server, host="127.0.0.1", port=8790):
    httpd = ThreadingHTTPServer((host, port), _http_handler_class(server))
    print("rMath MCP server (Streamable HTTP): http://%s:%d/mcp" % (host, port),
          flush=True)
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        httpd.server_close()
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description="rMath MCP server")
    parser.add_argument("--http", action="store_true",
                        help="serve Streamable HTTP instead of stdio")
    parser.add_argument("--host", default="127.0.0.1",
                        help="HTTP bind address (default: loopback only)")
    parser.add_argument("--port", type=int, default=8790,
                        help="HTTP port (default: 8790)")
    args = parser.parse_args(argv)

    server = McpServer()

    def _shutdown(*_):
        server.close()
        sys.exit(0)

    signal.signal(signal.SIGTERM, _shutdown)
    signal.signal(signal.SIGINT, _shutdown)

    try:
        if args.http:
            return serve_http(server, args.host, args.port)
        return serve_stdio(server)
    finally:
        server.close()


if __name__ == "__main__":
    sys.exit(main())
