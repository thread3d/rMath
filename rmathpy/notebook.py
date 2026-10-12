"""Zero-dependency web notebook server for the rMath kernel.

Usage: python3 -m rmathpy.notebook [--port 8787]
"""

import json
import os
import signal
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse

from .kernel import Kernel

_KERNEL = None
_KERNEL_LOCK = threading.RLock()


def get_kernel() -> Kernel:
    global _KERNEL
    with _KERNEL_LOCK:
        if _KERNEL is None:
            _KERNEL = Kernel()
        return _KERNEL


def work_dir() -> str:
    return os.environ.get("RMATH_WORKDIR") or os.getcwd()


def session_path() -> str:
    """Where the notebook's autosaved session lives (next to the scripts)."""
    return os.path.join(work_dir(), ".rmath-session.json")


def _balanced(text: str) -> bool:
    """True when brackets/braces/parens are balanced (strings respected)."""
    depth = 0
    i = 0
    while i < len(text):
        c = text[i]
        if c == '"':
            i += 1
            while i < len(text) and text[i] != '"':
                i += 2 if text[i] == "\\" else 1
            if i >= len(text):
                return False
        elif c in "[{(":
            depth += 1
        elif c in "]})":
            depth -= 1
            if depth < 0:
                return False
        i += 1
    return depth == 0


def fuzz_claim(kernel, lhs, rhs, variables, samples=5):
    """Numeric sanity check for a verified claim (backlog item 7).

    Substitutes random values for the free variables in both sides and compares
    the kernel's numeric results. A disagreement means the *translation* — not
    the mathematics — is suspect, so the caller downgrades the verdict. Returns
    "" when the samples agree (or cannot be evaluated)."""
    import random
    vals = [v for v in (variables or []) if v not in ("Pi", "E", "I")]
    if not vals or not rhs:
        return ""
    for _ in range(samples):
        env = {v: random.choice([-3, -2, -1, 1, 2, 3]) for v in vals}
        rules = ", ".join("%s -> %d" % (v, n) for v, n in env.items())
        try:
            a = kernel.eval("N[(%s) /. {%s}]" % (lhs, rules))
            b = kernel.eval("N[(%s) /. {%s}]" % (rhs, rules))
        except Exception:
            return ""
        av = (a.get("result") or "").strip()
        bv = (b.get("result") or "").strip()
        if a.get("status") != "ok" or b.get("status") != "ok" or not av or not bv:
            return ""
        if av != bv:
            return "at %s: %s vs %s" % (env, av, bv)
    return ""


def demo_expression(stmt: str) -> str:
    """The demo prints each example as ``Print["  <label>  =  ", expr]`` so that
    ``Get``/``make demo`` echoes every step. When a script is *opened* in the
    notebook, that statement is loaded as the bare ``expr`` instead, so each
    example gets a real ``Out[n]=`` line (and the label, already shown by the
    ``In[n]:=`` line, is not repeated). Anything not matching the convention is
    returned unchanged."""
    s = stmt.strip()
    if not s.startswith("Print["):
        return stmt
    i = len("Print[")
    while i < len(s) and s[i] in " \t\r\n":
        i += 1
    if i >= len(s) or s[i] != '"':
        return stmt
    j = i + 1
    while j < len(s):
        c = s[j]
        if c == "\\":
            j += 2
            continue
        if c == '"':
            break
        j += 1
    if j >= len(s) or "  =  " not in s[i + 1 : j]:
        return stmt
    k = j + 1
    while k < len(s) and s[k] in " \t\r\n":
        k += 1
    if k >= len(s) or s[k] != ",":
        return stmt
    body = s[k + 1 :].rstrip()
    if not body.endswith("]") or not _balanced(body[:-1]):
        return stmt
    expr = body[:-1].strip()
    return expr if expr else stmt


def handle_eval(body: dict) -> dict:
    text = body.get("input", "")
    with _KERNEL_LOCK:
        k = get_kernel()
        r = k.eval(text, steps=bool(body.get("steps")))
    out = dict(r)
    if r.get("graphics"):
        from .svg import render_graphics
        try:
            out["svg"] = render_graphics(r["graphics"])
        except Exception as e:
            out["svg_error"] = str(e)
    return out


# Hostnames the notebook is ever served from. A request naming any other Host,
# or carrying a cross-site Origin, did not come from our own page.
_LOCAL_HOSTNAMES = {"127.0.0.1", "localhost", "::1"}


def _split_host(value: str):
    """(hostname, port) from a `Host` header or an `Origin` authority."""
    value = (value or "").strip()
    if value.startswith("["):  # [::1]:8787
        host, _, rest = value[1:].partition("]")
        port = rest[1:] if rest.startswith(":") else ""
        return host.lower(), port
    host, sep, port = value.rpartition(":")
    if sep and port.isdigit():
        return host.lower(), port
    return value.lower(), ""


class Handler(BaseHTTPRequestHandler):
    def _origin_allowed(self) -> bool:
        """Reject cross-site and DNS-rebinding requests.

        A browser always sends `Host`, and on a POST it also sends `Origin` (and
        usually `Sec-Fetch-Site`), so a foreign Host, a non-local Origin or
        `Sec-Fetch-Site: cross-site` means the request did not come from the
        notebook page. Non-browser clients (curl, the app's helpers) send none of
        these and stay allowed, so the documented curl examples keep working.
        """
        port = str(self.server.server_address[1])
        host, host_port = _split_host(self.headers.get("Host"))
        if host and host not in _LOCAL_HOSTNAMES:
            return False
        if host_port and host_port != port:
            return False
        if (self.headers.get("Sec-Fetch-Site") or "").strip().lower() == "cross-site":
            return False
        origin = (self.headers.get("Origin") or "").strip()
        if origin:
            parsed = urlparse(origin)
            if parsed.scheme not in ("http", "https"):
                return False
            if (parsed.hostname or "").lower() not in _LOCAL_HOSTNAMES:
                return False
            if parsed.port and str(parsed.port) != port:
                return False
        return True

    def _guard(self) -> bool:
        if self._origin_allowed():
            return True
        self._send(403, "application/json",
                   b'{"status": "error", "error": "cross-site request refused"}')
        return False

    def log_message(self, fmt, *args):
        # Quiet by default; set RMATH_LOG_REQUESTS=1 to trace requests
        # (useful when diagnosing the app or a stuck page).
        if os.environ.get("RMATH_LOG_REQUESTS") == "1":
            sys.stderr.write("%s %s\n" % (self.address_string(), fmt % args))
            sys.stderr.flush()

    def _send(self, code, ctype, payload: bytes):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(payload)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(payload)

    def _json(self, obj):
        self._send(200, "application/json", json.dumps(obj).encode("utf-8"))

    def do_GET(self):
        if not self._guard():
            return
        path = urlparse(self.path).path
        if path == "/" or path == "/index.html":
            self._send(200, "text/html; charset=utf-8", index_html().encode("utf-8"))
        elif path == "/api":
            # The API describes itself: an agent can discover every route (and
            # the MCP entry point) without reading this file.
            from . import apispec
            self._json(apispec.index(self.server.server_address[1]))
        elif path == "/api/openapi.json":
            from . import apispec
            self._json(apispec.openapi(self.server.server_address[1]))
        elif path == "/api/names":
            with _KERNEL_LOCK:
                names = get_kernel().names()
            self._json({"names": names})
        elif path == "/api/health":
            self._json({"ok": True})
        elif path == "/api/lean/status":
            from . import lean
            self._json({"available": lean.available(),
                        "lake": lean.lake(),
                        "project": lean.project_dir(),
                        "toolchain": lean.toolchain(),
                        "server_mode": lean.persistent_mode(),
                        "server_trusted": lean.server_trusted()})
        elif path == "/api/agent/status":
            from . import harness
            url, source = harness.endpoint()
            from .paths import preflight
            from . import ollama_proxy
            self._json({"preflight": preflight(),
                        "thinking_proxy": ollama_proxy.stats() if ollama_proxy.running() else None,
                        "running": harness.current(),
                        "available": harness.available(),
                        "dsh": harness.find_dsh(),
                        "workspace": harness.workspace_root(),
                        "profile": harness.profile(),
                        "key": bool(harness.read_key()),
                        "endpoint": url,
                        "endpoint_source": source,
                        "ollama": harness.ollama_available(),
                        "ollama_models": harness.ollama_models(),
                        "model": harness.harness_model(),
                        "models": harness.available_models()})
        elif path.startswith("/katex/"):
            self.serve_katex(path[len("/katex/"):])
        else:
            self._send(404, "text/plain", b"not found")

    def serve_katex(self, rel: str):
        base = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                            "katex", "dist", "katex")
        target = os.path.normpath(os.path.join(base, rel))
        if not target.startswith(base) or not os.path.isfile(target):
            self._send(404, "text/plain", b"not found")
            return
        ctype = "application/javascript" if rel.endswith(".js") else (
            "text/css" if rel.endswith(".css") else
            "font/woff2" if rel.endswith(".woff2") else
            "font/woff" if rel.endswith(".woff") else
            "font/ttf" if rel.endswith(".ttf") else "application/octet-stream")
        with open(target, "rb") as f:
            data = f.read()
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "max-age=86400")
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        if not self._guard():
            return
        path = urlparse(self.path).path
        length = int(self.headers.get("Content-Length", 0))
        raw = self.rfile.read(length).decode("utf-8") if length else "{}"
        try:
            body = json.loads(raw) if raw else {}
        except Exception:
            body = {}

        if path == "/api/eval":
            try:
                self._json(handle_eval(body))
            except Exception as e:
                self._json({"status": "error", "error": str(e)})
        elif path == "/api/reset":
            with _KERNEL_LOCK:
                get_kernel().reset()
            self._json({"status": "ok"})
        elif path == "/api/split":
            # Split a multi-line input into statements with the kernel's own
            # splitter, which keeps one expression spanning lines together.
            with _KERNEL_LOCK:
                stmts = get_kernel().split(body.get("input", ""))
            self._json({"statements": stmts})
        elif path == "/api/save":
            self.api_save(body)
        elif path == "/api/open":
            self.api_open(body)
        elif path == "/api/save-svg":
            self.api_save_svg(body)
        elif path == "/api/session":
            self.api_session()
        elif path == "/api/autosave":
            self.api_autosave(body)
        elif path == "/api/agent":
            self.api_agent(body)
        elif path == "/api/agent/stream":
            self.api_agent_stream(body)
        elif path == "/api/agent/cancel":
            from . import acp, harness
            if acp.current():
                self._json(acp.cancel())
            else:
                self._json(harness.cancel_current())
        elif path == "/api/lean/check":
            from . import lean
            self._json(lean.run_lean(body.get("code", "")))
        elif path == "/api/lean/verify":
            from . import lean
            inp = body.get("input", "")
            result = body.get("result") or None
            result_form = None
            # `Steps[...]`/`Trace[...]` is a display wrapper, and `Solve[...]`
            # needs its roots in a structured form: for both, re-evaluate the
            # expression so the bridge gets the real value / InputForm (the
            # cell's displayed result is truncated OutputForm).
            inner = lean.unwrap_display(inp)
            query = inner if inner is not None else inp
            numeric = None
            if inner is not None or lean.is_solve(query):
                with _KERNEL_LOCK:
                    r = get_kernel().eval(query)
                    if r.get("status") == "ok":
                        plain = (r.get("result") or "").strip()
                        if plain and plain not in ("Null", "True", "False"):
                            result = plain
                            result_form = plain
                            if lean.is_solve(query):
                                # N[roots] tells the bridge which roots are real
                                # (Lean's ℝ check cannot use the complex ones).
                                rn = get_kernel().eval("N[" + plain + "]")
                                num = (rn.get("result") or "").strip()
                                numeric = num if num and num != plain else None
                inp = query
            d = lean.verify(inp, result, body.get("relation", "="),
                            result_form=result_form, numeric=numeric)
            # Item 7: numeric fuzz. Only for a plain equality claim, and only for
            # a verdict that would otherwise be reported as verified.
            if (d.get("outcome") == "verified" and result_form is None and result
                    and "==" not in inp and d.get("variables")):
                with _KERNEL_LOCK:
                    bad = fuzz_claim(get_kernel(), inp, result, d["variables"])
                if bad:
                    d["outcome"] = "cannot verify"
                    d["error"] = "numeric fuzz disagreed: " + bad
            self._json(d)
        elif path == "/api/lean/search":
            # Premise search (Loogle / LeanSearch). Offline is fine: an
            # unreachable backend comes back as an error with no results.
            from . import lean
            self._json(lean.premise_search(body.get("query", ""),
                                           body.get("backend", "loogle")))
        elif path == "/api/lean/explore":
            # Fast, *untrusted* diagnostics from the persistent LSP server
            # (Mathlib loads once). Verdicts still come from the one-shot run —
            # the server has published empty diagnostics for real errors before.
            from . import lean
            try:
                code, prop, ty, vs = lean.build_statement(
                    body.get("input", ""), body.get("result") or None,
                    result_form=body.get("result_form") or None)
            except ValueError as e:
                self._json({"status": "error", "error": str(e)})
                return
            try:
                diags, _, _ = lean.server().check(lean._finalized(code), timeout=60)
                self._json({"status": "ok", "proposition": prop, "type": ty,
                            "variables": vs, "diagnostics": diags,
                            "note": "exploration only — verdicts use the one-shot run"})
            except Exception as e:
                self._json({"status": "error", "error": str(e)})
        elif path == "/api/agent/key":
            from . import harness
            key = (body.get("key") or "").strip()
            if not key:
                self._json({"status": "error", "error": "empty key"})
            else:
                try:
                    self._json({"status": "ok",
                                "path": harness.store_key(key, body.get("base_url") or "")})
                except OSError as e:
                    self._json({"status": "error", "error": str(e)})
        elif path == "/api/agent/model":
            from . import acp, harness
            model = (body.get("model") or "").strip()
            try:
                stored = harness.store_model(model)
            except OSError as e:
                self._json({"status": "error", "error": str(e)})
                return
            # The ACP client reuses one session; restart it so the new model
            # takes effect on the next run (never kill a task in flight).
            try:
                if acp.available() and not acp.current():
                    acp.stop()
            except Exception:
                pass
            self._json({"status": "ok", "model": model, "path": stored})

        else:
            self._send(404, "text/plain", b"not found")

    # ---- harness (DeepSeek Harness) ----

    def build_agent_task(self, body: dict):
        from . import harness
        mode = body.get("mode", "raw")
        if mode == "cell":
            return harness.cell_task(
                body.get("input", ""), body.get("result", ""),
                body.get("messages", ""), body.get("instruction", ""))
        if mode == "fix":
            return harness.fix_task(body.get("report", ""), body.get("extra", ""))
        return body.get("task", "")

    def api_agent(self, body: dict):
        """Run one headless harness task and return its answer."""
        from . import harness
        task = self.build_agent_task(body)
        if not task.strip():
            self._json({"status": "error", "error": "empty task"})
            return
        r = harness.run_task(task)
        self._json(r)

    def api_agent_stream(self, body: dict):
        """Run a headless harness task, streaming its output as NDJSON."""
        from . import harness
        task = self.build_agent_task(body)
        if not task.strip():
            self._json({"status": "error", "error": "empty task"})
            return

        self.send_response(200)
        self.send_header("Content-Type", "application/x-ndjson")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Connection", "close")
        self.end_headers()
        lock = threading.Lock()

        def emit(obj):
            with lock:
                try:
                    self.wfile.write((json.dumps(obj) + "\n").encode("utf-8"))
                    self.wfile.flush()
                except Exception:
                    pass

        def on_line(text, kind="reasoning"):
            emit({"line": text, "kind": kind})

        r = None
        from . import acp
        if acp.available():
            try:
                r = acp.run_task(task, timeout=1800, on_line=on_line)
            except Exception as e:
                emit({"line": "ACP unavailable (%s); using the headless profile" % e,
                      "kind": "meta"})
                r = None
        if r is None:
            r = harness.run_task(task, on_line=on_line)
        emit({"done": True, "status": r.get("status"), "answer": r.get("answer", ""),
              "error": r.get("error", ""), "exit": r.get("exit"),
              "via": r.get("via", "headless"),
              "stop_reason": r.get("stop_reason"),
              "cancelled": r.get("cancelled", False) or r.get("stop_reason") == "cancelled"})

    # ---- notebook file operations (relative paths use the kernel work dir) ----

    def api_save(self, body: dict):
        inputs = body.get("inputs") or []
        name = body.get("name") or "notebook-%s.mt" % time.strftime("%Y%m%d-%H%M%S")
        path = os.path.join(work_dir(), name)
        try:
            with open(path, "w") as f:
                f.write("(* rMath notebook, saved %s *)\n\n"
                        % time.strftime("%Y-%m-%d %H:%M:%S"))
                for line in inputs:
                    f.write("%s\n" % line)
            self._json({"status": "ok", "path": path})
        except Exception as e:
            self._json({"status": "error", "error": str(e)})

    def api_open(self, body: dict):
        path = body.get("path", "")
        if not os.path.isabs(path):
            path = os.path.join(work_dir(), path)
        try:
            with open(path) as f:
                text = f.read()
            with _KERNEL_LOCK:
                statements = get_kernel().split(text)
            # The demo's labelled examples open as bare expressions, so they get
            # Out lines in the notebook (Get still prints the labelled line).
            statements = [demo_expression(s) for s in statements]
            if os.environ.get("RMATH_LOG_REQUESTS") == "1":
                sys.stderr.write("open %s -> %d statements, %d bytes\n"
                                 % (path, len(statements), len(text)))
                sys.stderr.flush()
            self._json({"status": "ok", "path": path, "text": text,
                        "statements": statements})
        except Exception as e:
            self._json({"status": "error", "error": str(e)})

    def api_save_svg(self, body: dict):
        svg = body.get("svg") or ""
        name = body.get("name") or "plot-%s.svg" % time.strftime("%Y%m%d-%H%M%S")
        path = os.path.join(work_dir(), name)
        try:
            with open(path, "w") as f:
                f.write(svg)
            self._json({"status": "ok", "path": path})
        except Exception as e:
            self._json({"status": "error", "error": str(e)})

    def api_session(self):
        """The autosaved cell inputs, so a restart does not lose the notebook."""
        try:
            with open(session_path()) as f:
                d = json.load(f)
        except (OSError, ValueError):
            self._json({"status": "ok", "inputs": [], "path": None})
            return
        self._json({"status": "ok", "inputs": d.get("inputs") or [],
                    "path": d.get("path")})

    def api_autosave(self, body: dict):
        inputs = body.get("inputs") or []
        path = session_path()
        try:
            if not inputs:
                try:
                    os.unlink(path)
                except OSError:
                    pass
            else:
                tmp = path + ".tmp"
                with open(tmp, "w") as f:
                    json.dump({"inputs": inputs, "path": body.get("path"),
                               "saved": time.strftime("%Y-%m-%d %H:%M:%S")}, f)
                os.replace(tmp, path)
        except OSError as e:
            self._json({"status": "error", "error": str(e)})
            return
        self._json({"status": "ok", "count": len(inputs)})


_STATIC_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "static")
_INDEX_CACHE = None


def index_html() -> str:
    """The notebook page, read once from `static/notebook.html`.

    Keeping the page in its own file leaves this module readable, instead of
    carrying a 1900-line string literal between the request handling and `main`.
    """
    global _INDEX_CACHE
    if _INDEX_CACHE is None:
        with open(os.path.join(_STATIC_DIR, "notebook.html"), encoding="utf-8") as f:
            _INDEX_CACHE = f.read()
    return _INDEX_CACHE





def main():
    import argparse
    parser = argparse.ArgumentParser(description="rMath web notebook")
    parser.add_argument("--port", type=int, default=8787)
    args = parser.parse_args()

    def _shutdown(*_):
        with _KERNEL_LOCK:
            if _KERNEL is not None:
                try:
                    _KERNEL.close()
                except Exception:
                    pass
        sys.exit(0)

    signal.signal(signal.SIGTERM, _shutdown)
    signal.signal(signal.SIGINT, _shutdown)

    # Warm the kernel so the first cell evaluates quickly.
    get_kernel()
    # Put the thinking proxy in front of a local Ollama: the harness's adapter
    # reads `reasoning_content`, Ollama sends `reasoning`, and without the rename
    # the panel shows nothing until the final answer.
    try:
        from . import harness, ollama_proxy
        if harness.ollama_available() and os.environ.get("RMATH_NO_THINKING_PROXY", "") not in ("1", "true"):
            url = ollama_proxy.start()
            if url:
                print("rMath thinking proxy: %s -> %s" % (url, ollama_proxy.UPSTREAM), flush=True)
    except Exception as e:
        print("rMath: thinking proxy not started: %s" % e, flush=True)

    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print("rMath notebook: http://127.0.0.1:%d" % args.port, flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
