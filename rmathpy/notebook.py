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
        else:
            self._send(404, "text/plain", b"not found")

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
  .row { display: flex; align-items: flex-start; min-height: 20px; }
  .lbl { flex: 0 0 auto; font-family: "SF Mono", Menlo, monospace; font-size: 12.5px;
         font-weight: 700; padding-top: 3px; }
  .inlbl { color: var(--inlabel); margin-right: 10px; }
  .outlbl { color: var(--outlabel); margin-right: 10px; }
  .content { flex: 1 1 auto; min-width: 0; font-family: "SF Mono", Menlo, Consolas, monospace;
             font-size: 13px; white-space: pre-wrap; word-break: break-word; line-height: 1.42; }
  .prints { color: #55606b; font-family: "SF Mono", Menlo, monospace; font-size: 12.5px;
            white-space: pre-wrap; padding: 1px 0 1px 2px; }
  .msgs { color: #a3221f; font-family: "SF Mono", Menlo, monospace; font-size: 12.5px;
          white-space: pre-wrap; padding: 1px 0 1px 2px; }
  .err { color: #a3221f; font-family: "SF Mono", Menlo, monospace; font-size: 12.5px;
         white-space: pre-wrap; }
  .tex { font-size: 14.5px; }
  .texrow { padding: 2px 0; }
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
             color: var(--inlabel); padding-top: 8px; min-width: 58px; }
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
  <button class="tb" title="Evaluate Input (Shift+Enter)" onclick="evaluate()">
    <svg viewBox="0 0 16 16" fill="currentColor"><path d="M4 2.5 13 8l-9 5.5z"/></svg></button>
  <button class="tb" title="Evaluate All Cells" onclick="evaluateAll()">
    <svg viewBox="0 0 16 16" fill="currentColor"><path d="M2 3 8 6.5 2 10zM8 3l6 3.5L8 10z"/></svg></button>
  <button class="tb" title="New Notebook (clear cells)" onclick="newNotebook()">
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M8 3.5v9M3.5 8h9"/></svg></button>
  <span class="sep"></span>
  <button class="tb" title="Open a script (.mt) into the notebook" onclick="openNotebook()">
    <svg viewBox="0 0 16 16" fill="currentColor"><path d="M1.5 3.5h4.2l1.4 1.6h7.4v7.4H1.5z"/></svg></button>
  <button class="tb" title="Save notebook inputs as .mt" onclick="saveNotebook()">
    <svg viewBox="0 0 16 16" fill="currentColor"><path d="M3 2h8.2L13 3.8V14H3z"/><path d="M5.4 2h5.2v3.6H5.4z" fill="#fff"/></svg></button>
  <button class="tb" title="Save last plot as SVG" onclick="saveLastPlotSvg()">
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M1.5 13.5h13M2.5 13V4M2.5 13l4-6 3 3 4-5"/></svg></button>
  <span class="sep"></span>
  <button class="tb" title="Reset kernel" onclick="resetKernel()">
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6"><path d="M13 8a5 5 0 1 1-1.7-3.8"/><path d="M13.4 2.2v3h-3"/></svg></button>
  <button class="tb" title="Zoom out" onclick="zoomOut()">
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.7"><circle cx="7" cy="7" r="4.2"/><path d="M10.2 10.2 14 14M5 7h4"/></svg></button>
  <button class="tb" title="Zoom in" onclick="zoomIn()">
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.7"><circle cx="7" cy="7" r="4.2"/><path d="M10.2 10.2 14 14M5 7h4M7 5v4"/></svg></button>
  <span class="sep"></span>
  <button class="tb wide" id="formbtn" title="Typesetting form (StandardForm / TraditionalForm)"
          onclick="toggleForm()">Standard</button>
  <span class="spacer"></span>
  <span class="brand">rMath</span>
  <button class="tb" title="Report a Bug (Shift+Cmd+B) — snapshot + diagnostics to the clipboard"
          onclick="location.href='rmath://report'">
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M4.5 7.5a3.5 3.5 0 1 1 7 0v3.2H4.5z"/><path d="M4.5 9H2.6M13.4 9H11.5M4.5 11.6 2.8 12.8M13.2 12.8l-1.7-1.2M6.6 3.6 6 2M9.4 3.6 10 2"/></svg></button>
  <button class="tb wide" title="Help (Cmd-?)" onclick="showHelp(true)">
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="8" cy="8" r="6"/><path d="M6.2 6.3a1.9 1.9 0 0 1 3.7.6c0 1.2-1.9 1.4-1.9 2.5"/><circle cx="8" cy="11.7" r=".8" fill="currentColor"/></svg>
    Help</button>
</div>

<div id="notebook"></div>

<div id="inputbar"><div class="inner">
  <div id="inlabel">In[1]:=</div>
  <textarea id="inp" rows="1" spellcheck="false"
    placeholder="Enter an expression, then press Shift+Enter to evaluate (Ctrl+Space completes)"></textarea>
  <button id="go" onclick="evaluate()">Evaluate</button>
</div></div>

<div id="help">
  <header>rMath Help<button onclick="showHelp(false)">Close</button></header>
  <div class="body">
    <div class="tips">
      <b>Shift+Enter</b> evaluates (or the Evaluate button) &middot; <b>Enter</b> adds a line to the
      cell &middot; <b>Ctrl+Space</b> completes &middot; pasting several lines makes several cells.<br>
      Load the demonstration: <code>Get["demo/demo.mt"]</code> &middot; inputs and outputs are
      typeset from the kernel's TeX printer.<br>
      Click any name below to insert it at the cursor.
    </div>
    <input id="fnfilter" placeholder="Filter functions…" oninput="filterFns()">
    <div id="fns"></div>
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
      try { katex.render(src, el, {throwOnError: false}); } catch (e) { el.textContent = src; }
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
async function evaluate() {
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

async function openNotebook(intoInput, givenPath) {
  const path = givenPath || window.prompt(
      'Open script (relative to the working directory):', 'demo/demo.mt');
  if (!path) return;
  const r = await fetch('/api/open', {method: 'POST',
    headers: {'Content-Type': 'application/json'}, body: JSON.stringify({path})});
  const resp = await r.json();
  if (resp.status !== 'ok') { toast('Open failed: ' + (resp.error || '')); return; }
  document.title = resp.path.split('/').pop() + ' — rMath';
  const statements = (resp.statements && resp.statements.length)
      ? resp.statements : resp.text.split('\n').filter(s => s.trim());
  if (intoInput) {
    // Editing mode: put the raw text in the input box.
    inp.value = resp.text;
    autoGrow();
    toast('Loaded ' + resp.path + ' into the input — Shift+Enter evaluates');
    inp.focus();
    return;
  }
  // Notebook mode: one cell per statement, evaluated in order.
  newNotebook();
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
      (summary ? '\n' + esc(summary) : '') + '</div>';
  toast('Bug report saved — path copied to clipboard');
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
    '<span onclick="insertText(\'' + n + '[\')">' + n + '</span>').join('');
}
function filterFns() { renderFns(document.getElementById('fnfilter').value); }

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
  if (e.key === 'Enter' && e.shiftKey) { e.preventDefault(); evaluate(); return; }
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
