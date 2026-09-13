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


def handle_eval(body: dict) -> dict:
    text = body.get("input", "")
    with _KERNEL_LOCK:
        k = get_kernel()
        r = k.eval(text)
    out = dict(r)
    if r.get("graphics"):
        from .svg import render_graphics
        try:
            out["svg"] = render_graphics(r["graphics"])
        except Exception as e:
            out["svg_error"] = str(e)
    return out


class Handler(BaseHTTPRequestHandler):
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
        path = urlparse(self.path).path
        if path == "/" or path == "/index.html":
            self._send(200, "text/html; charset=utf-8", INDEX_HTML.encode("utf-8"))
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
                        "toolchain": lean.toolchain()})
        elif path == "/api/agent/status":
            from . import harness
            url, source = harness.endpoint()
            self._json({"available": harness.available(),
                        "dsh": harness.find_dsh(),
                        "workspace": harness.workspace_root(),
                        "profile": harness.profile(),
                        "key": bool(harness.read_key()),
                        "endpoint": url,
                        "endpoint_source": source,
                        "ollama": harness.ollama_available(),
                        "ollama_models": harness.ollama_models(),
                        "model": harness.harness_model()})
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
        elif path == "/api/save":
            self.api_save(body)
        elif path == "/api/open":
            self.api_open(body)
        elif path == "/api/save-svg":
            self.api_save_svg(body)
        elif path == "/api/agent":
            self.api_agent(body)
        elif path == "/api/agent/stream":
            self.api_agent_stream(body)
        elif path == "/api/lean/check":
            from . import lean
            self._json(lean.run_lean(body.get("code", "")))
        elif path == "/api/lean/verify":
            from . import lean
            self._json(lean.verify(body.get("input", ""), body.get("result") or None,
                                   body.get("relation", "=")))
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

        r = harness.run_task(task, on_line=on_line)
        emit({"done": True, "status": r.get("status"), "answer": r.get("answer", ""),
              "error": r.get("error", ""), "exit": r.get("exit")})

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


INDEX_HTML = r"""<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>Untitled-1 — rMath</title>
<link rel="stylesheet" href="/katex/katex.min.css">
<script src="/katex/katex.min.js"></script>
<style>
  :root {
    --ink: #1a1a1a;
    --inlabel: #333333;
    --outlabel: #8a6d1f;
    --bracket: #c9c9c9;
    --accent: #2c5a8f;
  }
  * { box-sizing: border-box; }
  html, body { height: 100%; }
  body { margin: 0; background: #ffffff; color: var(--ink);
         font-family: "Helvetica Neue", Helvetica, Arial, sans-serif; font-size: 13px; }

  /* ---------------- ribbon / toolbar ---------------- */
  #ribbon { position: fixed; top: 0; left: 0; right: 0; height: 40px; z-index: 20;
            background: linear-gradient(#fbfbfb, #ececec); border-bottom: 1px solid #c4c4c4;
            display: flex; align-items: center; gap: 2px; padding: 0 8px;
            box-shadow: 0 1px 2px rgba(0,0,0,.06); }
  .tb { width: 30px; height: 28px; border: 1px solid transparent; border-radius: 4px;
        background: transparent; cursor: pointer; display: flex; align-items: center;
        justify-content: center; padding: 0; color: #3d3d3d; }
  .tb:hover { background: #e2e8f0; border-color: #c3ced9; }
  .tb:active { background: #d2dbe6; }
  .tb svg { width: 17px; height: 17px; }
  .tb.wide { width: auto; padding: 0 9px; font-size: 12px; gap: 5px; }
  .sep { width: 1px; height: 22px; background: #cfcfcf; margin: 0 6px; }
  #ribbon .spacer { flex: 1; }
  #ribbon .brand { font-size: 12px; color: #5b6b7c; margin-left: 4px;
                   font-weight: 600; letter-spacing: .3px; }

  /* ---------------- notebook ---------------- */
  #notebook { max-width: 940px; margin: 0 auto; padding: 56px 16px 130px; }
  .cell { position: relative; margin: 16px 0 18px; }
  .cellinner { margin-right: 26px; }
  .bracket { position: absolute; right: 4px; top: 1px; bottom: 1px; width: 7px;
             border: 1px solid var(--bracket); border-left: none;
             border-radius: 0 2px 2px 0; pointer-events: none; }
  .nobrackets .bracket { display: none; }
  /* Labels align on the first line's baseline. For a typeset fraction the
     baseline is the vinculum, so Out[n]= lines up with the fraction bar, as in
     Mathematica. Rows whose content is a plot have no text baseline and are
     aligned to the top instead (see .outrow.plot). */
  .row { display: flex; align-items: baseline; min-height: 20px; }
  .outrow.plot { align-items: flex-start; }
  .outrow.plot .lbl { margin-top: 3px; }
  /* In/Out labels: same font, size, weight and box; the colours differ (cool
     grey for In, warm gold for Out). The gold is deliberately dark — a lighter
     gold at this size reads as thinner, and therefore smaller. */
  .lbl { flex: 0 0 auto; font-family: "SF Mono", Menlo, monospace; font-size: 12.5px;
         font-weight: 700; padding: 2px 6px; border-radius: 3px; }
  .inlbl { color: #2f3a45; background: #e9edf1; margin-right: 9px; }
  .outlbl { color: #6b4c0a; background: #f2ecda; margin-right: 9px; }
  .content { flex: 1 1 auto; min-width: 0; font-family: "SF Mono", Menlo, Consolas, monospace;
             font-size: 13px; white-space: pre-wrap; word-break: break-word; line-height: 1.42; }
  .prints { color: #55606b; font-family: "SF Mono", Menlo, monospace; font-size: 12.5px;
            white-space: pre-wrap; padding: 1px 0 1px 2px; }
  .msgs { color: #a3221f; font-family: "SF Mono", Menlo, monospace; font-size: 12.5px;
          white-space: pre-wrap; padding: 1px 0 1px 2px; }
  .err { color: #a3221f; font-family: "SF Mono", Menlo, monospace; font-size: 12.5px;
         white-space: pre-wrap; }
  .tex { font-size: 15.5px; line-height: 1.7; }
  .texrow { padding: 3px 0; }
  .svgbox { margin: 4px 0 2px; }
  .svgbox svg { background: #fff; border: 1px solid #e2e2e2; }
  .outrow { margin-top: 3px; }
  .outrow.pending .lbl { color: #b0b0b0; }
  .pendingtext { color: #9a9a9a; font-style: italic; }

  /* ---------------- bottom input ---------------- */
  #inputbar { position: fixed; bottom: 0; left: 0; right: 0; background: #f4f4f4;
              border-top: 1px solid #c4c4c4; padding: 8px 0; z-index: 20; }
  #inputbar .inner { max-width: 940px; margin: 0 auto; padding: 0 16px;
                     display: flex; align-items: flex-start; gap: 8px; }
  #inlabel { font-family: "SF Mono", Menlo, monospace; font-weight: 700; font-size: 12.5px;
             color: #2f3a45; background: #e9edf1; padding: 7px 6px; border-radius: 3px;
             min-width: 62px; text-align: right; }
  #inp { flex: 1; font-family: "SF Mono", Menlo, Consolas, monospace; font-size: 13px;
         padding: 7px 9px; border: 1px solid #b9b9b9; border-radius: 3px; outline: 0;
         resize: none; overflow-y: auto; max-height: 110px; line-height: 1.4; background: #fff; }
  #inp:focus { border-color: var(--accent); box-shadow: 0 0 0 2px rgba(44,90,143,.12); }
  #go { background: linear-gradient(#4a7fb5, #2c5a8f); border: 1px solid #24486f; color: #fff;
        padding: 7px 16px; border-radius: 4px; cursor: pointer; font-size: 12.5px;
        font-weight: 600; }
  #go:hover { background: linear-gradient(#5489bf, #326099); }

  /* ---------------- help panel ---------------- */
  #help { position: fixed; top: 60px; bottom: 90px; left: 0; right: 0; margin: 0 auto;
          width: min(720px, 92vw); background: #fff; border: 1px solid #b9b9b9;
          border-radius: 6px; z-index: 40; box-shadow: 0 12px 40px rgba(0,0,0,.28);
          display: none; flex-direction: column; }
  #help.open { display: flex; }
  #help header { padding: 10px 14px; border-bottom: 1px solid #e2e2e2; font-weight: 700;
                 font-size: 14px; display: flex; align-items: center; gap: 10px; }
  #help header button { margin-left: auto; background: #eee; border: 1px solid #ccc;
                        border-radius: 4px; cursor: pointer; padding: 3px 9px; font-size: 12px; }
  #help .body { overflow: auto; padding: 12px 14px; }
  #help input { width: 100%; padding: 7px 9px; border: 1px solid #c5c5c5; border-radius: 4px;
                font-size: 13px; margin-bottom: 10px; outline: 0; }
  #fns { display: flex; flex-wrap: wrap; gap: 5px; }
  #fns span { font-family: "SF Mono", Menlo, monospace; font-size: 12px; padding: 2px 6px;
              border: 1px solid #dcdcdc; border-radius: 3px; cursor: pointer; background: #fafafa; }
  #fns span:hover { background: #eaf1f9; border-color: #9db8d4; }
  #help .tips { color: #4a5560; font-size: 12.5px; line-height: 1.7; margin-bottom: 12px; }
  #help .tips code { background: #f2f2f2; padding: 1px 4px; border-radius: 3px; }

  /* ---------------- harness (agent) panel ---------------- */
  /* Floating panels: movable by the header, resizable, collapsible, and placed
     so the notebook stays readable while both are open. Geometry is remembered
     per panel in localStorage. */
  #agent { position: fixed; left: 18px; bottom: 96px; width: min(520px, 44vw);
           height: min(58vh, 460px); background: #fff; border: 1px solid #b9b9b9;
           border-radius: 6px; z-index: 45; box-shadow: 0 10px 34px rgba(0,0,0,.28);
           display: none; flex-direction: column; overflow: hidden; }
  #agent.open { display: flex; }
  /* Resize affordances: a visible corner grip plus edge hot zones. The CSS
     `resize` property alone was unusable here — its handle sits underneath the
     panel's own scrollers, so a drag landed on a scrollbar instead. */
  .rz { position: absolute; right: 0; bottom: 0; width: 18px; height: 18px;
        cursor: nwse-resize; z-index: 6;
        background:
          linear-gradient(135deg, transparent 0 52%, #b6b6b6 52% 62%, transparent 62%),
          linear-gradient(135deg, transparent 0 70%, #b6b6b6 70% 80%, transparent 80%); }
  .rz:hover { background:
          linear-gradient(135deg, transparent 0 52%, #4a7fb5 52% 62%, transparent 62%),
          linear-gradient(135deg, transparent 0 70%, #4a7fb5 70% 80%, transparent 80%); }
  .rz-edge { position: absolute; z-index: 5; }
  .rz-right { top: 0; right: 0; width: 6px; height: 100%; cursor: ew-resize; }
  .rz-bottom { left: 0; bottom: 0; height: 6px; width: 100%; cursor: ns-resize; }
  #agent.collapsed .rz-right, #lean.collapsed .rz-right,
  #agent.collapsed .rz-bottom, #lean.collapsed .rz-bottom { display: none; }
  #agent.collapsed, #lean.collapsed { height: auto !important; }
  #agent.collapsed .body, #agent.collapsed #agentanswer,
  #lean.collapsed .body { display: none; }
  #agent header, #lean header { cursor: move; user-select: none; -webkit-user-select: none; }
  #agent header button, #lean header button { cursor: pointer; }
  #agent header .grip, #lean header .grip { margin-left: auto; display: flex; gap: 6px; }
  #agent header { padding: 9px 14px; border-bottom: 1px solid #e2e2e2; font-weight: 700;
                  font-size: 13.5px; display: flex; align-items: center; gap: 9px; }
  #agent header .who { color: #6b7280; font-weight: 400; font-size: 12px; }
  #agent header button { margin-left: auto; background: #eee; border: 1px solid #ccc;
                         border-radius: 4px; cursor: pointer; padding: 3px 9px; font-size: 12px; }
  #agent .body { padding: 10px 14px; display: flex; flex-direction: column; gap: 8px;
                 min-height: 0; flex: 1; }
  #agent textarea { width: 100%; height: 84px; font-family: "SF Mono", Menlo, monospace;
                    font-size: 12px; padding: 7px 8px; border: 1px solid #c5c5c5;
                    border-radius: 4px; outline: 0; resize: vertical; }
  #agent .row { display: flex; align-items: center; gap: 8px; }
  #agent .row button { background: linear-gradient(#4a7fb5, #2c5a8f); border: 1px solid #24486f;
                       color: #fff; padding: 6px 14px; border-radius: 4px; cursor: pointer;
                       font-size: 12px; font-weight: 600; }
  #agent .row button.ghost { background: #f3f3f3; color: #333; border-color: #ccc; font-weight: 400; }
  #agent .row .status { font-size: 11.5px; color: #6b7280; }
  #agentlog .l-reasoning { color: #8fa6b8; font-style: italic; }
  #agentlog .l-answer { color: #eaf3ec; }
  #agentanswer.md p { margin: 0 0 7px; }
  #agentanswer.md h1, #agentanswer.md h2, #agentanswer.md h3 { margin: 9px 0 5px; line-height: 1.25; }
  #agentanswer.md h1 { font-size: 16px; } #agentanswer.md h2 { font-size: 14.5px; }
  #agentanswer.md h3 { font-size: 13.5px; }
  #agentanswer.md ul, #agentanswer.md ol { margin: 4px 0 7px; padding-left: 22px; }
  #agentanswer.md li { margin: 2px 0; }
  #agentanswer.md code { background: #f2f0ea; border: 1px solid #e3e0d6; border-radius: 3px;
                         padding: 0 3px; font-size: 12px; }
  #agentanswer.md pre.mdcode { background: #14181d; color: #e6ecef; border-radius: 4px;
                               padding: 8px 10px; overflow: auto; margin: 5px 0 8px; position: relative; }
  #agentanswer.md pre.mdcode code { background: none; border: 0; padding: 0; color: inherit;
                                    font-size: 11.5px; line-height: 1.45; }
  #agentanswer.md pre.mdcode[data-lang]:before { content: attr(data-lang); position: absolute;
        top: 0; right: 6px; font-size: 10px; color: #7d8b93; }
  #agentanswer.md blockquote { margin: 5px 0 8px; padding: 3px 10px; border-left: 3px solid #d6d2c4;
                               color: #4b5563; background: #faf9f5; }
  #agentanswer.md hr { border: 0; border-top: 1px solid #e2ded2; margin: 9px 0; }
  #agentanswer.md a { color: #2c5a8f; }
  #agentanswer.md strong { font-weight: 700; }
  #agentlog { flex: 0 1 auto; max-height: 30vh; min-height: 74px; overflow: auto; background: #14181d; color: #cfe3d4;
              font-family: "SF Mono", Menlo, monospace; font-size: 11.5px; line-height: 1.45;
              padding: 8px 10px; border-radius: 4px; white-space: pre-wrap; }
  #agentanswer { border-top: 1px solid #e2e2e2; padding: 9px 14px; flex: 0 1 auto;
                 max-height: 46vh; overflow: auto; font-size: 12.5px; display: none;
                 font-family: -apple-system, "Segoe UI", system-ui, sans-serif;
                 color: #1f2937; background: #fdfdfb; }
  #agentanswer:not(.md) { white-space: pre-wrap; font-family: "SF Mono", Menlo, monospace; }
  #agentanswer.show { display: block; }
  .asklink { color: #2c5a8f; cursor: pointer; text-decoration: underline dotted; }

  /* ---------------- Lean panel ---------------- */
  #lean { position: fixed; right: 18px; top: 62px; width: min(520px, 44vw);
          height: min(58vh, 460px); background: #fff; border: 1px solid #b9b9b9;
          border-radius: 6px; z-index: 46; box-shadow: 0 10px 34px rgba(0,0,0,.28);
          display: none; flex-direction: column; overflow: hidden; }
  #lean.open { display: flex; }
  #lean header { padding: 9px 14px; border-bottom: 1px solid #e2e2e2; font-weight: 700;
                 font-size: 13.5px; display: flex; align-items: center; gap: 9px; }
  #lean header .who { color: #6b7280; font-weight: 400; font-size: 12px; }
  #lean header button { margin-left: auto; background: #eee; border: 1px solid #ccc;
                        border-radius: 4px; cursor: pointer; padding: 3px 9px; font-size: 12px; }
  #lean .body { padding: 10px 14px; display: flex; flex-direction: column; gap: 8px;
                min-height: 0; flex: 1; overflow: auto; }
  #lean textarea { width: 100%; height: 104px; font-family: "SF Mono", Menlo, monospace;
                   font-size: 12px; padding: 7px 8px; border: 1px solid #c5c5c5;
                   border-radius: 4px; outline: 0; resize: vertical; }
  #lean .row { display: flex; align-items: center; gap: 8px; }
  #lean .row button { background: linear-gradient(#4a7fb5, #2c5a8f); border: 1px solid #24486f;
                      color: #fff; padding: 6px 14px; border-radius: 4px; cursor: pointer;
                      font-size: 12px; font-weight: 600; }
  #lean .row button.ghost { background: #f3f3f3; color: #333; border-color: #ccc; font-weight: 400; }
  #lean .row .status { font-size: 11.5px; color: #6b7280; }
  #leanstmt:empty { display: none; }
  #leanstmt { border: 1px solid #e6e2d6; border-radius: 4px; padding: 8px 10px; background: #fbfaf6; }
  #leandiag { flex: 0 1 auto; min-height: 42px; max-height: 46vh; overflow: auto; font-family: "SF Mono", Menlo, monospace;
              font-size: 11.5px; line-height: 1.5; }
  #leandiag .d { padding: 3px 7px; border-radius: 3px; margin-bottom: 4px; white-space: pre-wrap; }
  #leandiag .error { background: #fdecec; color: #8c1c1c; border-left: 3px solid #c0392b; }
  #leandiag .warning { background: #fdf6e3; color: #7a5d10; border-left: 3px solid #d4a017; }
  #leandiag .info { background: #eef4fa; color: #2c4a6b; border-left: 3px solid #4a7fb5; }
  #leandiag .ok { background: #eaf6ec; color: #1e5b2a; border-left: 3px solid #2e8b57; font-weight: 600; }

  /* ---------------- toast / autocomplete ---------------- */
  #toast { position: fixed; bottom: 78px; left: 0; right: 0; text-align: center; z-index: 60;
           pointer-events: none; }
  #toast div { display: inline-block; background: rgba(35,45,55,.93); color: #fff;
               padding: 7px 14px; border-radius: 16px; font-size: 12.5px; }
  #ac { position: absolute; background: #fff; border: 1px solid #aaa; max-height: 190px;
        overflow: auto; z-index: 50; display: none; font-size: 12.5px;
        font-family: "SF Mono", Menlo, monospace; box-shadow: 0 6px 18px rgba(0,0,0,.16); }
  #ac div { padding: 3px 10px; cursor: pointer; }
  #ac div.sel, #ac div:hover { background: #e8f0fe; }
</style>
</head>
<body>
<div id="ribbon">
  <button class="tb" data-action="evalInput" title="Evaluate Input (Shift+Enter)">
    <svg viewBox="0 0 16 16" fill="currentColor"><path d="M4 2.5 13 8l-9 5.5z"/></svg></button>
  <button class="tb" data-action="evalAll" title="Evaluate All Cells">
    <svg viewBox="0 0 16 16" fill="currentColor"><path d="M2 3 8 6.5 2 10zM8 3l6 3.5L8 10z"/></svg></button>
  <button class="tb" data-action="newNotebook" title="New Notebook (clear cells)">
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M8 3.5v9M3.5 8h9"/></svg></button>
  <span class="sep"></span>
  <button class="tb" data-action="openNotebook" title="Open a script (.mt) into the notebook">
    <svg viewBox="0 0 16 16" fill="currentColor"><path d="M1.5 3.5h4.2l1.4 1.6h7.4v7.4H1.5z"/></svg></button>
  <button class="tb" data-action="saveNotebook" title="Save notebook inputs as .mt">
    <svg viewBox="0 0 16 16" fill="currentColor"><path d="M3 2h8.2L13 3.8V14H3z"/><path d="M5.4 2h5.2v3.6H5.4z" fill="#fff"/></svg></button>
  <button class="tb" data-action="saveLastPlotSvg" title="Save last plot as SVG">
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M1.5 13.5h13M2.5 13V4M2.5 13l4-6 3 3 4-5"/></svg></button>
  <span class="sep"></span>
  <button class="tb" data-action="resetKernel" title="Reset kernel">
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6"><path d="M13 8a5 5 0 1 1-1.7-3.8"/><path d="M13.4 2.2v3h-3"/></svg></button>
  <button class="tb" data-action="zoomOut" title="Zoom out">
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.7"><circle cx="7" cy="7" r="4.2"/><path d="M10.2 10.2 14 14M5 7h4"/></svg></button>
  <button class="tb" data-action="zoomIn" title="Zoom in">
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.7"><circle cx="7" cy="7" r="4.2"/><path d="M10.2 10.2 14 14M5 7h4M7 5v4"/></svg></button>
  <span class="sep"></span>
  <button class="tb wide" id="formbtn" title="Typesetting form (StandardForm / TraditionalForm)"
          >Standard</button>
  <span class="spacer"></span>
  <span class="brand">rMath</span>
  <button class="tb" data-action="showLean" title="Check Lean 4 code, or verify the last cell in Lean">
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4"><path d="M2.5 3.5h11v9h-11z"/><path d="M5 6.5h6M5 9h3.5"/><path d="M10.5 9.5l1 1 2-2.4"/></svg></button>
  <button class="tb" data-action="showAgent" title="Ask the DeepSeek Harness (it can read and edit this project)">
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4"><rect x="3" y="5" width="10" height="7.5" rx="1.6"/><path d="M6.2 8.2v1.2M9.8 8.2v1.2M8 5V3M4.6 3.4 3.6 2.2M11.4 3.4l1-1.2"/></svg></button>
  <button class="tb" id="reportbtn"
          title="Report a Bug (Shift+Cmd+B) — snapshot + diagnostics to the clipboard">
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M4.5 7.5a3.5 3.5 0 1 1 7 0v3.2H4.5z"/><path d="M4.5 9H2.6M13.4 9H11.5M4.5 11.6 2.8 12.8M13.2 12.8l-1.7-1.2M6.6 3.6 6 2M9.4 3.6 10 2"/></svg></button>
  <button class="tb wide" data-action="showHelp" title="Help (Cmd-?)">
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="8" cy="8" r="6"/><path d="M6.2 6.3a1.9 1.9 0 0 1 3.7.6c0 1.2-1.9 1.4-1.9 2.5"/><circle cx="8" cy="11.7" r=".8" fill="currentColor"/></svg>
    Help</button>
</div>

<div id="notebook"></div>

<div id="inputbar"><div class="inner">
  <div id="inlabel">In[1]:=</div>
  <textarea id="inp" rows="1" spellcheck="false"
    placeholder="Enter an expression, then press Shift+Enter to evaluate (Ctrl+Space completes)"></textarea>
  <button id="go">Evaluate</button>
</div></div>

<div id="help">
  <header>rMath Help<button id="helpclose">Close</button></header>
  <div class="body">
    <div class="tips">
      <b>Shift+Enter</b> evaluates (or the Evaluate button) &middot; <b>Enter</b> adds a line to the
      cell &middot; <b>Ctrl+Space</b> completes &middot; pasting several lines makes several cells.<br>
      Load the demonstration: <code>Get["demo/demo.mt"]</code> &middot; inputs and outputs are
      typeset from the kernel's TeX printer.<br>
      Click any name below to insert it at the cursor.
    </div>
    <input id="fnfilter" placeholder="Filter functions…">
    <div id="fns"></div>
  </div>
</div>
<div id="agent">
  <header>&#129302; Harness <span class="who" id="agentwho"></span>
    <span class="grip"><button class="collapse" data-for="agent" title="Collapse">&#9660;</button>
    <button id="agentclose">Close</button></span></header>
  <div class="body">
    <textarea id="agenttask" spellcheck="false"
      placeholder="What should the harness do? (it runs with this project as its workspace root and can read, edit and run commands)"></textarea>
    <div class="row">
      <button id="agentsend">Send to harness</button>
      <button id="agentcell" class="ghost">Ask about the last cell</button>
      <button id="agentkey" class="ghost">Set API key…</button>
      <span class="status" id="agentstatus"></span>
    </div>
    <div id="agentlog"></div>
  </div>
  <div id="agentanswer"></div>
</div>
<div id="lean">
  <header>&#129518; Lean <span class="who" id="leanwho"></span>
    <span class="grip"><button class="collapse" data-for="lean" title="Collapse">&#9660;</button>
    <button id="leanclose">Close</button></span></header>
  <div class="body">
    <textarea id="leancode" spellcheck="false"
      placeholder="Lean 4 (Mathlib is imported automatically), e.g.&#10;example (x : ℝ) : x^2 + 2*x + 1 = (x + 1)^2 := by ring"></textarea>
    <div class="row">
      <button id="leancheck">Check</button>
      <button id="leancell" class="ghost">Verify the last cell</button>
      <span class="status" id="leanstatus"></span>
    </div>
    <div id="leanstmt" class="md"></div>
    <div id="leandiag"></div>
  </div>
</div>
<div id="ac"></div>
<div id="toast"></div>

<script>
const notebook = document.getElementById('notebook');
const inp = document.getElementById('inp');
const ac = document.getElementById('ac');
const inlabel = document.getElementById('inlabel');
let history = JSON.parse(localStorage.getItem('rmath-history') || '[]');
let histPos = history.length;
let NAMES = [];
let inCounter = 0;
let evaluated = [];
let lastPlotSvg = null;

fetch('/api/names').then(r => r.json()).then(d => { NAMES = d.names || []; });

function esc(s) {
  return (s + '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}
function escAttr(s) {
  return (s + '').replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');
}
// Two TeX renderings per expression (StandardForm / TraditionalForm); the
// Format menu switches between them instantly, without re-evaluating.
let currentForm = localStorage.getItem('rmath-form') || 'standard';
function mathSpan(texStd, texTrad) {
  return '<span class="tex" data-tex-std="' + escAttr(texStd || '') +
         '" data-tex-trad="' + escAttr(texTrad || '') + '"></span>';
}
function texFor(el) {
  const std = el.dataset.texStd || '', trad = el.dataset.texTrad || '';
  return (currentForm === 'traditional') ? (trad || std) : (std || trad);
}
function typesetAll(root) {
  if (!root || !root.querySelectorAll) return;
  root.querySelectorAll('span.tex').forEach(el => {
    const src = texFor(el);
    if (!src) return;
    if (window.katex) {
      try {
        katex.render(src, el, {throwOnError: true});
      } catch (e) {
        // Don't dump raw TeX for the whole cell on a parse error: render what
        // KaTeX can and let it mark just the offending part.
        try {
          katex.render(src, el, {throwOnError: false, strict: false});
        } catch (e2) {
          el.textContent = src;
        }
      }
      el.dataset.done = '1';
    } else { el.textContent = src; }
  });
}
function setForm(form) {
  currentForm = (form === 'traditional') ? 'traditional' : 'standard';
  localStorage.setItem('rmath-form', currentForm);
  const b = document.getElementById('formbtn');
  if (b) b.textContent = (currentForm === 'traditional') ? 'Traditional' : 'Standard';
  typesetAll(document);
  toast('Typesetting form: ' + b.textContent + 'Form');
}
function toggleForm() {
  setForm(currentForm === 'traditional' ? 'standard' : 'traditional');
}
function toast(msg) {
  const t = document.getElementById('toast');
  t.innerHTML = '<div>' + esc(msg) + '</div>';
  clearTimeout(toast._t);
  toast._t = setTimeout(() => { t.innerHTML = ''; }, 4000);
}
function setInLabel() { inlabel.textContent = 'In[' + (inCounter + 1) + ']:='; }

function addCell() {
  const cell = document.createElement('div');
  cell.className = 'cell';
  cell.innerHTML = '<div class="cellinner">' +
      '<div class="row inrow"><div class="lbl inlbl"></div>' +
      '<div class="content in-text"></div></div>' +
      '<div class="row outrow pending"><div class="lbl outlbl"></div>' +
      '<div class="content out-text"><span class="pendingtext">Evaluating…</span></div></div>' +
      '</div><div class="bracket"></div>';
  notebook.appendChild(cell);
  return cell;
}

async function evaluateLine(input, cell) {
  const inRow = cell.querySelector('.inrow');
  const outRow = cell.querySelector('.outrow');
  const inContent = inRow.querySelector('.content');
  const outContent = outRow.querySelector('.content');
  const inLbl = inRow.querySelector('.lbl');
  const outLbl = outRow.querySelector('.lbl');
  inCounter += 1;
  inLbl.textContent = 'In[' + inCounter + ']:=';
  outLbl.textContent = 'Out[' + inCounter + ']=';
  inContent.textContent = input;

  let resp;
  try {
    const r = await fetch('/api/eval', {method: 'POST',
      headers: {'Content-Type': 'application/json'},
      body: JSON.stringify({input})});
    resp = await r.json();
  } catch (e) {
    outContent.innerHTML = '<div class="err">' + esc(String(e)) + '</div>';
    outLbl.textContent = '';
    outRow.classList.remove('pending');
    return;
  }
  const inN = (typeof resp.in === 'number') ? resp.in : inCounter;
  inCounter = inN;
  inLbl.textContent = 'In[' + inN + ']:=';
  setInLabel();
  if (resp.tex_in) {
    inContent.innerHTML = '<div class="texrow">' +
        mathSpan(resp.tex_in, resp.tex_in_trad) + '</div>';
    inContent.title = input;
  }

  let html = '';
  if (resp.status === 'error') {
    html = '<div class="err">' + esc(resp.error || 'error') + '</div>';
  } else {
    if (resp.prints && resp.prints.length)
      html += '<div class="prints">' + resp.prints.map(esc).join('\n') + '</div>';
    if (resp.messages && resp.messages.length)
      html += '<div class="msgs">' + resp.messages.map(m => esc(m.text)).join('\n') + '</div>';
    if (resp.svg) {
      html += '<div class="svgbox">' + resp.svg + '</div>';
      outRow.classList.add('plot');
      lastPlotSvg = resp.svg;
    } else if (resp.tex) {
      html += mathSpan(resp.tex, resp.tex_trad);
    } else if (resp.result && resp.result.trim()) {
      html += esc(resp.result);
    }
  }
  outContent.innerHTML = html;
  cell.dataset.result = (resp.result || '').slice(0, 400);
  cell.dataset.status = resp.status || '';
  outRow.classList.remove('pending');
  // Mathematica semantics: Out[n] carries the *input* number and is shown only
  // when the evaluation produced a value (Null results get no Out label).
  const hasValue = (typeof resp.out === 'number');
  outLbl.textContent = (resp.status === 'error' || !hasValue) ? '' : 'Out[' + inN + ']=';
  typesetAll(cell);
  evaluated.push(input);
}

let lastCell = null;
async function evalInput() {
  const raw = inp.value;
  if (!raw.trim()) return;
  const lines = raw.split('\n').map(s => s.trim()).filter(s => s.length);
  inp.value = '';
  autoGrow();
  ac.style.display = 'none';
  for (const line of lines) {
    history.push(line);
    const cell = addCell();
    lastCell = cell;
    await evaluateLine(line, cell);
  }
  localStorage.setItem('rmath-history', JSON.stringify(history.slice(-200)));
  histPos = history.length;
  if (lastCell) lastCell.scrollIntoView({block: 'end', behavior: 'smooth'});
  inp.focus();
  autoGrow();
}

async function evaluateAll() {
  const inputs = evaluated.slice();
  if (!inputs.length) return;
  for (const line of inputs) {
    const cell = addCell();
    lastCell = cell;
    await evaluateLine(line, cell);
  }
  if (lastCell) lastCell.scrollIntoView({block: 'end', behavior: 'smooth'});
}

function newNotebook() {
  notebook.innerHTML = '';
  evaluated = [];
  lastPlotSvg = null;
  inCounter = 0;
  setInLabel();
  resetKernelQuiet();
  document.title = 'Untitled-1 — rMath';
  toast('New notebook');
}
async function resetKernelQuiet() {
  try { await fetch('/api/reset', {method: 'POST'}); } catch (e) {}
}
async function resetKernel() {
  await resetKernelQuiet();
  inCounter = 0;
  setInLabel();
  const cell = addCell();
  cell.querySelector('.inrow').querySelector('.lbl').textContent = '';
  const outRow = cell.querySelector('.outrow');
  outRow.classList.remove('pending');
  outRow.querySelector('.lbl').textContent = '';
  outRow.querySelector('.content').innerHTML =
      '<div class="prints">Kernel reset — In/Out numbering restarted.</div>';
}

let lastAction = null;

async function openNotebook(intoInput, givenPath) {
  const path = givenPath || window.prompt(
      'Open script (relative to the working directory):', 'demo/demo.mt');
  if (!path) return;
  let resp;
  try {
    const r = await fetch('/api/open', {method: 'POST',
      headers: {'Content-Type': 'application/json'}, body: JSON.stringify({path})});
    resp = await r.json();
  } catch (e) {
    lastAction = {action: 'open', path: path, error: String(e)};
    toast('Open failed: ' + e);
    return;
  }
  if (resp.status !== 'ok') {
    lastAction = {action: 'open', path: path, error: resp.error || 'error'};
    toast('Open failed: ' + (resp.error || ''));
    return;
  }

  // Statements come from the kernel's splitter; fall back to a local split so
  // an RPC hiccup cannot leave an empty notebook behind.
  let statements = resp.statements || [];
  if (!statements.length && resp.text) {
    statements = resp.text.split('\n')
        .map(s => s.trim())
        .filter(s => s.length && !s.startsWith('(*') && !s.startsWith('#'));
    lastAction = {action: 'open', path: path, statements: statements.length,
                  note: 'used the local line splitter'};
  } else {
    lastAction = {action: 'open', path: path, statements: statements.length};
  }

  if (intoInput) {
    inp.value = resp.text || '';
    autoGrow();
    toast('Loaded ' + resp.path + ' into the input — Shift+Enter evaluates');
    inp.focus();
    return;
  }

  if (!statements.length) {
    // Nothing to open: report it and leave the notebook alone.
    toast('Open: no statements found in ' + resp.path + ' (notebook unchanged)');
    lastAction.error = 'no statements';
    return;
  }

  const existing = notebook.querySelectorAll('.cell').length;
  if (existing > 0 && !window.confirm(
      'Open ' + resp.path + '? This replaces the ' + existing +
      ' cell(s) in the current notebook.')) {
    lastAction = {action: 'open', path: path, cancelled: true};
    toast('Open cancelled — notebook unchanged');
    return;
  }

  newNotebook();
  document.title = resp.path.split('/').pop() + ' — rMath';
  toast('Opened ' + resp.path + ' — ' + statements.length + ' cells');
  for (const stmt of statements) {
    history.push(stmt);
    const cell = addCell();
    lastCell = cell;
    await evaluateLine(stmt, cell);
  }
  localStorage.setItem('rmath-history', JSON.stringify(history.slice(-200)));
  histPos = history.length;
  if (lastCell) lastCell.scrollIntoView({block: 'end', behavior: 'smooth'});
}

async function saveNotebook() {
  const r = await fetch('/api/save', {method: 'POST',
    headers: {'Content-Type': 'application/json'},
    body: JSON.stringify({inputs: evaluated})});
  const resp = await r.json();
  toast(resp.status === 'ok' ? 'Saved to ' + resp.path : 'Save failed: ' + (resp.error || ''));
}

async function saveLastPlotSvg() {
  if (!lastPlotSvg) { toast('No plot in this session yet'); return; }
  const r = await fetch('/api/save-svg', {method: 'POST',
    headers: {'Content-Type': 'application/json'}, body: JSON.stringify({svg: lastPlotSvg})});
  const resp = await r.json();
  toast(resp.status === 'ok' ? 'Plot saved to ' + resp.path : 'Save failed: ' + (resp.error || ''));
}

function insertText(t) {
  const start = inp.selectionStart, end = inp.selectionEnd;
  inp.value = inp.value.slice(0, start) + t + inp.value.slice(end);
  inp.selectionStart = inp.selectionEnd = start + t.length;
  inp.focus();
  autoGrow();
}

let zoom = 1;
function applyZoom() { document.documentElement.style.zoom = zoom; }
function zoomIn() { zoom = Math.min(2, Math.round((zoom + 0.1) * 10) / 10); applyZoom(); }
function zoomOut() { zoom = Math.max(0.5, Math.round((zoom - 0.1) * 10) / 10); applyZoom(); }
function zoomReset() { zoom = 1; applyZoom(); }
function toggleBrackets() { notebook.classList.toggle('nobrackets'); }
function focusInput() { inp.focus(); }

/// Everything the native side needs for a bug report: the recent transcript,
/// the last error, and the current UI state.
function reportContext() {
  const cells = [];
  const nodes = notebook.querySelectorAll('.cell');
  const start = Math.max(0, nodes.length - 12);
  for (let i = start; i < nodes.length; i++) {
    const cell = nodes[i];
    const inLbl = cell.querySelector('.inlbl');
    const outLbl = cell.querySelector('.outlbl');
    const inC = cell.querySelector('.inrow .content');
    const outC = cell.querySelector('.outrow .content');
    const err = cell.querySelector('.err');
    const msgs = cell.querySelector('.msgs');
    const prints = cell.querySelector('.prints');
    const hasSvg = !!cell.querySelector('.svgbox');
    cells.push({
      in: inLbl ? inLbl.textContent : '',
      input: inC ? (inC.title || inC.textContent) : '',
      out: outLbl ? outLbl.textContent : '',
      result: cell.dataset.result || '',
      error: err ? err.textContent : '',
      messages: msgs ? msgs.textContent : '',
      prints: prints ? prints.textContent.slice(0, 400) : '',
      plot: hasSvg,
    });
  }
  const bad = [...nodes].reverse().find(c => c.querySelector('.err') || c.querySelector('.msgs'));
  const lastError = bad ? {
    input: (bad.querySelector('.inrow .content') || {}).title ||
           (bad.querySelector('.inrow .content') || {}).textContent || '',
    error: (bad.querySelector('.err') || {}).textContent || '',
    messages: (bad.querySelector('.msgs') || {}).textContent || '',
  } : null;
  return JSON.stringify({
    title: document.title, form: currentForm, cells: cells, lastError: lastError,
    evaluated: evaluated.slice(-12),
    // the input box often explains a "nothing happened" report
    inputBox: (inp.value || '').slice(0, 2000),
    inputBoxLines: (inp.value || '').split('\n').length,
    lastAction: lastAction,
  });
}

/// Show where the report went, and keep the path selectable in the notebook.
function showReport(path, summary) {
  const cell = addCell();
  cell.querySelector('.inrow').querySelector('.lbl').textContent = 'Report';
  const outRow = cell.querySelector('.outrow');
  outRow.classList.remove('pending');
  outRow.querySelector('.lbl').textContent = '';
  outRow.querySelector('.content').innerHTML =
      '<div class="prints">Bug report written to ' + esc(path) +
      '\n(path and summary are on the clipboard — paste them into the chat)' +
      (summary ? '\n' + esc(summary) : '') +
      '\n<span class="asklink" data-fix="' + escAttr(path) + '">' +
      'Ask the harness to fix this</span></div>';
  const link = outRow.querySelector('.asklink');
  if (link) link.addEventListener('click', function () { askFixReport(link.dataset.fix); });
  toast('Bug report saved — path copied to clipboard');
}


// ---------------- minimal markdown for harness answers ----------------
// Self-contained (no new dependency). Input is HTML-escaped *first*, then the
// formatting is applied, so a model answer can never inject markup.

function mdEscape(s) {
  return String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;")
                  .replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

function mdInline(s) {
  // Math first, as a placeholder element rendered by KaTeX afterwards.
  s = s.replace(/\$([^$\n]+)\$/g, function (m, tex) {
    return '<span class="mdmath" data-tex="' + mdEscape(tex) + '"></span>';
  });
  s = s.replace(/`([^`]+)`/g, "<code>$1</code>");
  s = s.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  s = s.replace(/(^|[\s(])\*([^*\n]+)\*/g, "$1<em>$2</em>");
  s = s.replace(/(^|[\s(])_([^_\n]+)_/g, "$1<em>$2</em>");
  s = s.replace(/~~([^~]+)~~/g, "<del>$1</del>");
  s = s.replace(/\[([^\]]+)\]\((https?:[^)\s]+)\)/g,
                '<a href="$2" target="_blank" rel="noreferrer">$1</a>');
  return s;
}

function mdToHtml(src) {
  const lines = String(src == null ? "" : src).split("\n");
  let html = "", list = null, para = [];
  const closeList = function () { if (list) { html += "</" + list + ">"; list = null; } };
  const flushPara = function () { if (para.length) { html += "<p>" + mdInline(para.join(" ")) + "</p>"; para = []; } };
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const fence = line.match(/^\s*```(\w*)\s*$/);
    if (fence) {
      flushPara(); closeList();
      const buf = []; i++;
      while (i < lines.length && !/^\s*```\s*$/.test(lines[i])) { buf.push(lines[i]); i++; }
      i++;
      html += '<pre class="mdcode"' + (fence[1] ? ' data-lang="' + mdEscape(fence[1]) + '"' : "") +
              "><code>" + mdEscape(buf.join("\n")) + "</code></pre>";
      continue;
    }
    const h = line.match(/^(#{1,6})\s+(.*)$/);
    if (h) {
      flushPara(); closeList();
      const n = h[1].length;
      html += "<h" + n + ">" + mdInline(mdEscape(h[2])) + "</h" + n + ">";
      i++; continue;
    }
    if (/^\s*([-*_])\1{2,}\s*$/.test(line)) { flushPara(); closeList(); html += "<hr>"; i++; continue; }
    if (/^\s*>\s?/.test(line)) {
      flushPara(); closeList();
      const buf = [];
      while (i < lines.length && /^\s*>\s?/.test(lines[i])) { buf.push(lines[i].replace(/^\s*>\s?/, "")); i++; }
      html += "<blockquote>" + mdToHtml(buf.join("\n")) + "</blockquote>";
      continue;
    }
    const li = line.match(/^\s*([-*+]|\d+[.)])\s+(.*)$/);
    if (li) {
      flushPara();
      const want = /^\d/.test(li[1]) ? "ol" : "ul";
      if (list !== want) { closeList(); html += "<" + want + ">"; list = want; }
      html += "<li>" + mdInline(mdEscape(li[2])) + "</li>";
      i++; continue;
    }
    if (!line.trim()) { flushPara(); closeList(); i++; continue; }
    para.push(mdEscape(line.trim()));
    i++;
  }
  flushPara(); closeList();
  return html;
}

/// Typeset any $...$ left as placeholders by mdInline.
function mdTypeset(root) {
  if (!window.katex) return;
  root.querySelectorAll(".mdmath").forEach(function (el) {
    try { katex.render(el.dataset.tex, el, {throwOnError: false, displayMode: false}); }
    catch (e) { el.textContent = el.dataset.tex; }
  });
}

/// Render markdown into a container (escaping first, always).
function mdRenderInto(el, text) {
  el.innerHTML = mdToHtml(text);
  mdTypeset(el);
}

// ---------------- floating panels ----------------
// Drag by the header, resize from the corner (CSS), collapse with the chevron,
// and remember the geometry per panel. Clicking a panel brings it to the front,
// so the notebook, Lean and Harness can all be read at once.

let __zTop = 44;

function panelGeometry(el) {
  const r = el.getBoundingClientRect();
  return {left: Math.round(r.left), top: Math.round(r.top),
          width: Math.round(r.width), height: Math.round(r.height), z: el.style.zIndex};
}

function savePanel(el, key) {
  try { localStorage.setItem("rmath.panel." + key, JSON.stringify(panelGeometry(el))); } catch (e) {}
}

function restorePanel(el, key) {
  let g = null;
  try { g = JSON.parse(localStorage.getItem("rmath.panel." + key) || "null"); } catch (e) {}
  if (!g) return;
  // keep it on screen even if the window shrank
  const w = Math.min(g.width, window.innerWidth - 40);
  const h = Math.min(g.height, window.innerHeight - 120);
  el.style.width = w + "px";
  el.style.height = h + "px";
  el.style.left = Math.max(8, Math.min(g.left, window.innerWidth - w - 8)) + "px";
  el.style.top = Math.max(8, Math.min(g.top, window.innerHeight - h - 90)) + "px";
  el.style.right = "auto";
  el.style.bottom = "auto";
}

/// Forget the remembered geometry and put the panels back to their defaults.
function resetPanelLayout() {
  try {
    localStorage.removeItem("rmath.panel.lean");
    localStorage.removeItem("rmath.panel.agent");
  } catch (e) {}
  [["lean", "right", "top"], ["agent", "left", "bottom"]].forEach(function (spec) {
    const el = document.getElementById(spec[0]);
    if (!el) return;
    el.style.width = "";
    el.style.height = "";
    el.style.left = spec[1] === "left" ? "18px" : "auto";
    el.style.right = spec[1] === "right" ? "18px" : "auto";
    el.style.top = spec[2] === "top" ? "62px" : "auto";
    el.style.bottom = spec[2] === "bottom" ? "96px" : "auto";
  });
}

function bringToFront(el) {
  __zTop += 1;
  el.style.zIndex = String(__zTop);
}

/// Add the corner grip and edge hot zones that resize a panel.
function makeResizable(el, key) {
  const grip = document.createElement("div");
  grip.className = "rz";
  grip.title = "Resize";
  el.appendChild(grip);
  const right = document.createElement("div");
  right.className = "rz-edge rz-right";
  const bottom = document.createElement("div");
  bottom.className = "rz-edge rz-bottom";
  el.appendChild(right);
  el.appendChild(bottom);

  const startResize = function (ev, dirs) {
    ev.preventDefault();
    ev.stopPropagation();
    bringToFront(el);
    const r = el.getBoundingClientRect();
    const sx = ev.clientX, sy = ev.clientY, sw = r.width, sh = r.height;
    const move = function (e) {
      let w = dirs.indexOf("e") >= 0 ? sw + (e.clientX - sx) : sw;
      let h = dirs.indexOf("s") >= 0 ? sh + (e.clientY - sy) : sh;
      w = Math.max(300, Math.min(w, window.innerWidth - 16));
      h = Math.max(150, Math.min(h, window.innerHeight - 16));
      el.style.width = w + "px";
      el.style.height = h + "px";
    };
    const up = function () {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      savePanel(el, key);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };
  grip.addEventListener("pointerdown", function (e) { startResize(e, "se"); });
  right.addEventListener("pointerdown", function (e) { startResize(e, "e"); });
  bottom.addEventListener("pointerdown", function (e) { startResize(e, "s"); });
}

/// Make a fixed-position panel draggable and remembered.
function makeFloating(el, key) {
  if (!el) return;
  restorePanel(el, key);
  const header = el.querySelector("header");
  el.addEventListener("pointerdown", function () { bringToFront(el); });
  if (header) {
    header.addEventListener("pointerdown", function (ev) {
      if (ev.target.closest("button")) return;      // let the buttons work
      bringToFront(el);
      const r = el.getBoundingClientRect();
      const offX = ev.clientX - r.left, offY = ev.clientY - r.top;
      el.style.right = "auto"; el.style.bottom = "auto";
      el.style.left = r.left + "px"; el.style.top = r.top + "px";
      const move = function (e) {
        const x = Math.max(4, Math.min(e.clientX - offX, window.innerWidth - 60));
        const y = Math.max(4, Math.min(e.clientY - offY, window.innerHeight - 40));
        el.style.left = x + "px"; el.style.top = y + "px";
      };
      const up = function () {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
        savePanel(el, key);
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
      ev.preventDefault();
    });
    header.addEventListener("dblclick", function (ev) {
      if (!ev.target.closest("button")) el.classList.toggle("collapsed");
    });
  }
  const grip = el.querySelector("button.collapse");
  if (grip) grip.addEventListener("click", function () {
    el.classList.toggle("collapsed");
    grip.innerHTML = el.classList.contains("collapsed") ? "&#9650;" : "&#9660;";
    savePanel(el, key);
  });
  // remember the size when the corner is dragged (CSS resize has no event)
  el.addEventListener("pointerup", function () { savePanel(el, key); });
  makeResizable(el, key);
  bringToFront(el);
}

// ---------------- Lean panel ----------------

function leanStatus(text) { document.getElementById("leanstatus").textContent = text; }

function leanShowDiagnostics(res) {
  const box = document.getElementById("leandiag");
  box.innerHTML = "";
  (res.diagnostics || []).forEach(function (d) {
    const div = document.createElement("div");
    div.className = "d " + d.severity;
    div.textContent = d.severity + " (line " + d.line + ", col " + d.col + "): " + d.message;
    box.appendChild(div);
  });
  if (res.error) {
    const div = document.createElement("div");
    div.className = "d error";
    div.textContent = res.error;
    box.appendChild(div);
  }
  if (!(res.diagnostics || []).length && !res.error) {
    const div = document.createElement("div");
    div.className = "d ok";
    div.textContent = "Lean accepted the code with no errors or warnings.";
    box.appendChild(div);
  }
}

function showLean(task) {
  document.getElementById("lean").classList.add("open");
  document.getElementById("leancode").focus();
  fetch("/api/lean/status").then(r => r.json()).then(s => {
    document.getElementById("leanwho").textContent = s.available
      ? ((s.toolchain || "Lean") + "  ·  " + s.project)
      : "Lean not found — install elan/lake";
  }).catch(() => {});
}
function hideLean() { document.getElementById("lean").classList.remove("open"); }

async function leanCheck(code) {
  leanStatus("checking…");
  const stmt = document.getElementById("leanstmt");
  stmt.innerHTML = "";
  try {
    const r = await fetch("/api/lean/check", {method: "POST",
      headers: {"Content-Type": "application/json"}, body: JSON.stringify({code: code})});
    const res = await r.json();
    leanShowDiagnostics(res);
    leanStatus(res.status === "ok" ? "checked" : "errors");
  } catch (e) { leanStatus("failed: " + e); }
}

async function leanVerifyLastCell() {
  const cells = [...notebook.querySelectorAll(".cell")];
  let input = inp.value, result = "";
  const cell = cells[cells.length - 1];
  if (cell && !input) {
    const inC = cell.querySelector(".inrow .content");
    input = (inC && inC.title) || "";
    result = cell.dataset.result || "";
  }
  if (!input) { leanStatus("nothing to verify — evaluate a cell first"); return; }
  leanStatus("translating…");
  try {
    const r = await fetch("/api/lean/verify", {method: "POST",
      headers: {"Content-Type": "application/json"},
      body: JSON.stringify({input: input, result: result})});
    const res = await r.json();
    const stmt = document.getElementById("leanstmt");
    if (res.lean) {
      mdRenderInto(stmt, "```lean\n" + res.lean.trim() + "\n```");
    } else {
      stmt.innerHTML = "";
    }
    leanShowDiagnostics(res);
    const badge = res.outcome === "verified" ? "VERIFIED by Lean"
                : res.outcome === "unproved" ? "NOT PROVED" : "cannot verify";
    leanStatus(badge + (res.type ? "  (" + res.type + ")" : ""));
    if (res.outcome === "verified") {
      const box = document.getElementById("leandiag");
      const div = document.createElement("div");
      div.className = "d ok";
      div.textContent = "✓ " + (res.proposition || "") + "   — proved in " + (res.type || "") +
                        (res.variables && res.variables.length ? " with " + res.variables.join(", ") : "");
      box.insertBefore(div, box.firstChild);
    }
  } catch (e) { leanStatus("failed: " + e); }
}

// ---------------- harness (agent) panel ----------------

function agentLog(text, kind) {
  const box = document.getElementById("agentlog");
  const span = document.createElement("span");
  span.className = kind === "answer" ? "l-answer" : "l-reasoning";
  span.textContent = text + "\n";
  box.appendChild(span);
  box.scrollTop = box.scrollHeight;
}
function agentStatus(text) { document.getElementById("agentstatus").textContent = text; }
function agentAnswer(text) {
  const a = document.getElementById("agentanswer");
  a.innerHTML = "";
  a.classList.remove("md");
  if (text) {
    try { mdRenderInto(a, text); a.classList.add("md"); }
    catch (e) { a.textContent = text; }   // never lose the answer to a render bug
  }
  a.classList.toggle("show", !!text);
}
function showAgent(task) {
  const panel = document.getElementById("agent");
  panel.classList.add("open");
  if (typeof task === "string") document.getElementById("agenttask").value = task;
  document.getElementById("agentwho").textContent = "DeepSeek Harness";
  document.getElementById("agenttask").focus();
  fetch("/api/agent/status").then(r => r.json()).then(s => {
    document.getElementById("agentwho").textContent = s.available
      ? ("dsh " + s.profile + " → " + s.endpoint + " (" + s.endpoint_source + ")" +
         (s.model ? ", model " + s.model : "") +
         (s.ollama ? ", ollama has " + s.ollama_models.length + " model(s)" : "") +
         (s.key ? "" : "  — no API key: use “Set API key…” or export DEEPSEEK_API_KEY"))
      : "dsh not found — set DSH_BIN";
  }).catch(() => {});
}
function hideAgent() { document.getElementById("agent").classList.remove("open"); }

/// Ask about the most recent cell: its input, output and messages.
function askAboutCell() {
  const cells = [...notebook.querySelectorAll(".cell")];
  const cell = cells[cells.length - 1];
  let input = inp.value, result = "", messages = "";
  if (!input && cell) {
    const inC = cell.querySelector(".inrow .content");
    input = (inC && (inC.title || inC.textContent)) || "";
    result = cell.dataset.result || "";
    const m = cell.querySelector(".msgs");
    messages = m ? m.textContent.trim().replace(/\s+/g, " ") : "";
  }
  showAgent("");
  const body = {mode: "cell", input: input, result: result, messages: messages,
                instruction: document.getElementById("agenttask").value.trim()};
  runAgent(body);
}

/// Ask the harness to fix the bug a report describes.
function askFixReport(path) {
  showAgent("");
  document.getElementById("agenttask").value =
    "Fix the bug described in " + path + " (read the report first).";
  runAgent({mode: "fix", report: path});
}

async function runAgent(body) {
  const log = document.getElementById("agentlog");
  log.textContent = "";
  agentAnswer("");
  const send = document.getElementById("agentsend");
  send.disabled = true;
  agentStatus("running…");
  try {
    const r = await fetch("/api/agent/stream", {method: "POST",
      headers: {"Content-Type": "application/json"}, body: JSON.stringify(body)});
    if (!r.ok || !r.body) { agentStatus("failed: HTTP " + r.status); return; }
    const reader = r.body.getReader();
    const dec = new TextDecoder();
    let buf = "";
    for (;;) {
      const {value, done} = await reader.read();
      if (done) break;
      buf += dec.decode(value, {stream: true});
      let nl;
      while ((nl = buf.indexOf("\n")) >= 0) {
        const line = buf.slice(0, nl); buf = buf.slice(nl + 1);
        if (!line.trim()) continue;
        let ev; try { ev = JSON.parse(line); } catch (e) { continue; }
        if (ev.line) agentLog(ev.line, ev.kind);
        if (ev.done) {
          agentStatus(ev.status === "ok" ? "done" : ("failed: " + (ev.error || ev.status)));
          agentAnswer(ev.answer || "(no answer)");
        }
      }
    }
  } catch (e) {
    agentStatus("failed: " + e);
  } finally {
    send.disabled = false;
  }
}

function showHelp(open) {
  const panel = document.getElementById('help');
  const want = (open === undefined) ? true : !!open;
  panel.classList.toggle('open', want);
  if (want) { renderFns(''); document.getElementById('fnfilter').focus(); }
  else { inp.focus(); }
}
function renderFns(filter) {
  const box = document.getElementById('fns');
  const f = (filter || '').toLowerCase();
  const list = NAMES.filter(n => !f || n.toLowerCase().includes(f)).slice(0, 400);
  box.innerHTML = list.map(n =>
    '<span data-insert="' + n + '[">' + n + '</span>').join('');
  box.querySelectorAll('span[data-insert]').forEach(el => {
    el.addEventListener('click', function () { insertText(el.dataset.insert); });
  });
}
function filterFns() { renderFns(document.getElementById('fnfilter').value); }

/// Talk to the desktop app when it is hosting this page. Returns false in a
/// plain browser (where a custom URL scheme would only make the system try to
/// open an unknown application).
function nativeBridge(action, payload) {
  const handlers = window.webkit && window.webkit.messageHandlers;
  const h = handlers && handlers.rmath;
  if (!h) return false;
  const msg = Object.assign({action: action}, payload || {});
  h.postMessage(msg);
  return true;
}

function reportBug() {
  if (!nativeBridge('report')) {
    toast('Bug reports need the rMath app (Help ▸ Report a Bug), or run: make report');
  }
}

// Wire the ribbon/help buttons here: inline onclick handlers are resolved
// through element -> form -> document -> window, so a global named like a DOM
// member (e.g. evaluate, which collides with document.evaluate) would be
// shadowed and silently do nothing.
function wireControls() {
  const actions = {
    evalInput: evalInput,
    evalAll: evaluateAll,
    newNotebook: newNotebook,
    openNotebook: function () { openNotebook(false); },
    saveNotebook: saveNotebook,
    saveLastPlotSvg: saveLastPlotSvg,
    resetKernel: resetKernel,
    zoomOut: zoomOut,
    zoomIn: zoomIn,
    showHelp: function () { showHelp(true); },
    showAgent: function () { showAgent(""); },
    showLean: function () { showLean(); },
    resetPanelLayout: function () { resetPanelLayout(); },
  };
  document.querySelectorAll('#ribbon .tb').forEach(b => {
    const name = b.dataset.action;
    if (name && actions[name]) b.addEventListener('click', actions[name]);
  });
  const form = document.getElementById('formbtn');
  if (form) form.addEventListener('click', toggleForm);
  const report = document.getElementById('reportbtn');
  if (report) {
    report.addEventListener('click', reportBug);
    // In a browser the app-only controls cannot work; say so up front.
    if (!(window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.rmath)) {
      report.title = 'Report a Bug — available in the rMath desktop app';
      report.style.opacity = '0.45';
    }
  }
  const go = document.getElementById('go');
  if (go) go.addEventListener('click', evalInput);
  const agentSend = document.getElementById('agentsend');
  if (agentSend) agentSend.addEventListener('click', function () {
    runAgent({task: document.getElementById('agenttask').value});
  });
  const agentCell = document.getElementById('agentcell');
  if (agentCell) agentCell.addEventListener('click', askAboutCell);
  const agentKey = document.getElementById('agentkey');
  if (agentKey) agentKey.addEventListener('click', async function () {
    const key = window.prompt(
      'API key for the harness (stored outside the repository, mode 0600):', '');
    if (!key) return;
    const base = window.prompt(
      'Endpoint base URL — blank for the DeepSeek default (https://api.deepseek.com),\n' +
      'or any OpenAI-compatible gateway:', '');
    const r = await fetch('/api/agent/key', {method: 'POST',
      headers: {'Content-Type': 'application/json'},
      body: JSON.stringify({key: key, base_url: base || ''})});
    const resp = await r.json();
    agentStatus(resp.status === 'ok' ? ('key stored in ' + resp.path) : ('failed: ' + resp.error));
  });
  const agentClose = document.getElementById('agentclose');
  if (agentClose) agentClose.addEventListener('click', hideAgent);
  const leanCheckBtn = document.getElementById('leancheck');
  if (leanCheckBtn) leanCheckBtn.addEventListener('click', function () {
    leanCheck(document.getElementById('leancode').value);
  });
  const leanCellBtn = document.getElementById('leancell');
  if (leanCellBtn) leanCellBtn.addEventListener('click', leanVerifyLastCell);
  const leanClose = document.getElementById('leanclose');
  if (leanClose) leanClose.addEventListener('click', hideLean);
  makeFloating(document.getElementById('agent'), 'agent');
  makeFloating(document.getElementById('lean'), 'lean');
  const helpClose = document.getElementById('helpclose');
  if (helpClose) helpClose.addEventListener('click', function () { showHelp(false); });
  const filter = document.getElementById('fnfilter');
  if (filter) filter.addEventListener('input', filterFns);
}
wireControls();

function autoGrow() {
  inp.style.height = 'auto';
  inp.style.height = Math.min(inp.scrollHeight, 110) + 'px';
}
inp.addEventListener('input', autoGrow);

function currentWord() {
  const before = inp.value.slice(0, inp.selectionStart);
  const m = before.match(/[A-Za-z$]+$/);
  return m ? m[0] : '';
}
function acOpen() { return ac.style.display === 'block'; }
let acIndex = 0, acItems = [];
function showComplete() {
  const word = currentWord();
  if (!word || !NAMES.length) return;
  acItems = NAMES.filter(n => n.startsWith(word)).slice(0, 30);
  if (!acItems.length) return;
  acIndex = 0;
  const rect = inp.getBoundingClientRect();
  ac.innerHTML = acItems.map((n, i) =>
    '<div class="' + (i === 0 ? 'sel' : '') + '" data-n="' + n + '">' + n + '</div>').join('');
  ac.style.left = rect.left + 'px';
  ac.style.top = Math.max(46, rect.top - Math.min(acItems.length * 21 + 4, 190)) + 'px';
  ac.style.display = 'block';
  ac.querySelectorAll('div').forEach(d => { d.onclick = () => insertComplete(d.dataset.n); });
}
function acMove(delta) {
  if (!acOpen()) return;
  acIndex = (acIndex + delta + acItems.length) % acItems.length;
  ac.querySelectorAll('div').forEach((d, i) => d.classList.toggle('sel', i === acIndex));
}
function insertComplete(name) {
  const pos = inp.selectionStart;
  const before = inp.value.slice(0, pos).replace(/[A-Za-z$]+$/, name);
  inp.value = before + inp.value.slice(inp.selectionStart);
  ac.style.display = 'none';
  inp.focus();
}

inp.addEventListener('keydown', e => {
  if (acOpen()) {
    if (e.key === 'ArrowDown') { acMove(1); e.preventDefault(); return; }
    if (e.key === 'ArrowUp') { acMove(-1); e.preventDefault(); return; }
    if (e.key === 'Enter' || e.key === 'Tab') {
      insertComplete(acItems[acIndex]); e.preventDefault(); return;
    }
    if (e.key === 'Escape') { ac.style.display = 'none'; return; }
  }
  if (e.key === 'Enter' && e.shiftKey) { e.preventDefault(); evalInput(); return; }
  // plain Enter inserts a newline (multi-line cells), as in Mathematica
  else if (e.key === 'ArrowUp' && !inp.value && history.length) {
    histPos = Math.max(0, histPos - 1);
    inp.value = history[histPos] || ''; autoGrow(); e.preventDefault();
  } else if (e.key === 'ArrowDown' && !inp.value && history.length) {
    histPos = Math.min(history.length, histPos + 1);
    inp.value = history[histPos] || ''; autoGrow(); e.preventDefault();
  } else if (e.key === ' ' && e.ctrlKey) { e.preventDefault(); showComplete(); }
});
document.addEventListener('click', e => {
  if (!ac.contains(e.target)) ac.style.display = 'none';
});
document.addEventListener('keydown', e => {
  if (e.key === 'Escape') { ac.style.display = 'none'; showHelp(false); }
  if (e.key === '?' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); showHelp(true); }
});

setInLabel();
setForm(currentForm);
inp.focus();
autoGrow();
</script>
</body>
</html>"""


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
    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print("rMath notebook: http://127.0.0.1:%d" % args.port, flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
