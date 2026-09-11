"""Zero-dependency web notebook server for the rMath kernel.

Usage: python3 -m rmathpy.notebook [--port 8787]
"""

import json
import threading
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
    def log_message(self, fmt, *args):  # quiet
        pass

    def _send(self, code, ctype, payload: bytes):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(payload)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(payload)

    def do_GET(self):
        path = urlparse(self.path).path
        if path == "/" or path == "/index.html":
            self._send(200, "text/html; charset=utf-8", INDEX_HTML.encode("utf-8"))
        elif path == "/api/names":
            with _KERNEL_LOCK:
                names = get_kernel().names()
            self._send(200, "application/json", json.dumps({"names": names}).encode())
        elif path == "/api/health":
            self._send(200, "application/json", b'{"ok": true}')
        else:
            self._send(404, "text/plain", b"not found")

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
                self._send(200, "application/json", json.dumps(handle_eval(body)).encode())
            except Exception as e:
                self._send(
                    200,
                    "application/json",
                    json.dumps({"status": "error", "error": str(e)}).encode(),
                )
        elif path == "/api/reset":
            with _KERNEL_LOCK:
                get_kernel().reset()
            self._send(200, "application/json", b'{"status": "ok"}')
        else:
            self._send(404, "text/plain", b"not found")


INDEX_HTML = r"""<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>rMath</title>
<style>
  :root { --accent: #2a58b2; --accent2: #b43c3c; }
  * { box-sizing: border-box; }
  body { font-family: system-ui, -apple-system, sans-serif; background: #fafafa;
         margin: 0; color: #222; }
  #bar { background: #2c3e50; color: #fff; padding: 8px 16px; display: flex;
         align-items: center; gap: 12px; }
  #bar h1 { font-size: 15px; margin: 0; font-weight: 600; letter-spacing: .5px; }
  #bar .dot { width: 9px; height: 9px; border-radius: 50%; background: #777; }
  #bar .dot.on { background: #4caf50; }
  #bar button { margin-left: auto; background: #3a5570; color: #fff; border: 0;
                padding: 5px 12px; border-radius: 4px; cursor: pointer; font-size: 12px; }
  #bar button:hover { background: #3d5a77; }
  #cells { max-width: 860px; margin: 18px auto; padding: 0 12px 90px; }
  .cell { margin-bottom: 10px; }
  .in-label { font-size: 11px; color: #2a58b2; font-weight: 600; margin-bottom: 2px; }
  .in-text { background: #fff; border: 1px solid #e2e2e2; padding: 7px 10px;
             font-family: ui-monospace, Menlo, monospace; font-size: 13px;
             white-space: pre-wrap; border-radius: 3px; }
  .out-label { font-size: 11px; color: #a4762a; font-weight: 600; margin: 2px 0; }
  .out-text { font-family: ui-monospace, Menlo, monospace; font-size: 13px;
              white-space: pre-wrap; padding: 2px 0; }
  .prints { color: #666; font-family: ui-monospace, Menlo, monospace; font-size: 12px;
            white-space: pre-wrap; margin-top: 2px; }
  .msgs { color: var(--accent2); font-size: 12px; font-family: ui-monospace, Menlo,
          monospace; margin-top: 2px; }
  .err { color: var(--accent2); font-family: ui-monospace, Menlo, monospace;
         font-size: 12px; white-space: pre-wrap; }
  .svgbox { margin: 6px 0 2px; }
  .svgbox svg { background: #fff; border: 1px solid #e0e0e0; }
  #inputbar { position: fixed; bottom: 0; left: 0; right: 0; background: #eee;
              border-top: 1px solid #ddd; padding: 10px 0; }
  #inputbar .inner { max-width: 860px; margin: 0 auto; padding: 0 12px;
                     display: flex; gap: 8px; align-items: flex-end; }
  #inp { flex: 1; font-family: ui-monospace, Menlo, monospace; font-size: 13px;
         padding: 9px 10px; border: 1px solid #c9c9c9; border-radius: 4px; outline: 0;
         resize: none; overflow: hidden; max-height: 180px; line-height: 1.35; }
  #inp:focus { border-color: #2a58b2; }
  #go { background: #2a58b2; border: 0; color: #fff; padding: 9px 18px;
        border-radius: 4px; cursor: pointer; font-size: 13px; }
  #go:hover { background: #1d469b; }
  .hint { color: #999; font-size: 11px; margin: 4px 2px; }
  #ac { position: absolute; background: #fff; border: 1px solid #bbb; max-height: 180px;
        overflow: auto; z-index: 10; display: none; font-size: 12px;
        font-family: ui-monospace, Menlo, monospace; }
  #ac div { padding: 3px 10px; cursor: pointer; }
  #ac div:hover { background: #e8f0fe; }
</style>
</head>
<body>
  <div id="bar">
    <span class="dot" id="status"></span>
    <h1>rMath</h1>
    <button id="reset">Reset kernel</button>
  </div>
  <div id="cells"></div>
  <div id="inputbar"><div class="inner">
    <textarea id="inp" rows="1" placeholder="Enter an expression; press Enter to evaluate (Shift+Enter = new line; Ctrl+Space completes)" spellcheck="false"></textarea>
    <button id="go">Evaluate</button>
  </div></div>
  <div id="ac"></div>
<script>
const cells = document.getElementById('cells');
const inp = document.getElementById('inp');
const ac = document.getElementById('ac');
const status = document.getElementById('status');
let history = JSON.parse(localStorage.getItem('rmath-history') || '[]');
let histPos = history.length;
let NAMES = [];

fetch('/api/names').then(r => r.json()).then(d => { NAMES = d.names || []; });
fetch('/api/health').then(() => status.classList.add('on')).catch(() => {});

function esc(s) {
  return (s + '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function addCellHTML(html) {
  const d = document.createElement('div');
  d.className = 'cell';
  d.innerHTML = html;
  cells.appendChild(d);
  return d;
}

async function evaluateLine(input, outEl) {
  try {
    const r = await fetch('/api/eval', {method: 'POST',
      headers: {'Content-Type': 'application/json'},
      body: JSON.stringify({input})});
    const resp = await r.json();
    let html = '';
    if (resp.status === 'error') {
      html = '<div class="err">' + esc(resp.error || 'error') + '</div>';
    } else {
      if (resp.prints && resp.prints.length)
        html += '<div class="prints">' + resp.prints.map(esc).join('\n') + '</div>';
      if (resp.messages && resp.messages.length)
        html += '<div class="msgs">' + resp.messages.map(m => esc(m.text)).join('\n') + '</div>';
      if (resp.svg) {
        html += '<div class="out-label">Out[' + (resp.out ?? '') + ']=</div>' +
                '<div class="svgbox">' + resp.svg + '</div>';
      } else if (resp.result && resp.result.trim()) {
        html += '<div class="out-label">Out[' + (resp.out ?? '') + ']=</div>' +
                '<div class="out-text">' + esc(resp.result) + '</div>';
      }
    }
    outEl.innerHTML = html;
  } catch (e) {
    outEl.innerHTML = '<div class="err">' + esc(String(e)) + '</div>';
  }
}

function autoGrow() {
  inp.style.height = 'auto';
  inp.style.height = Math.min(inp.scrollHeight, 180) + 'px';
}
inp.addEventListener('input', autoGrow);

async function evaluate() {
  const raw = inp.value;
  if (!raw.trim()) return;
  // Multiple pasted lines become multiple notebook cells.
  const lines = raw.split('\n').map(s => s.trim()).filter(s => s.length);
  inp.value = '';
  autoGrow();
  ac.style.display = 'none';
  for (const line of lines) {
    history.push(line);
    addCellHTML('<div class="in-label">In</div><div class="in-text">' +
                esc(line) + '</div>');
    const out = addCellHTML('<div class="out-label">Evaluating...</div>');
    await evaluateLine(line, out);
  }
  localStorage.setItem('rmath-history', JSON.stringify(history.slice(-200)));
  histPos = history.length;
  cells.appendChild(inp);  // keep the input last
  inp.focus();
  autoGrow();
}

function acOpen() { return ac.style.display === 'block'; }
inp.addEventListener('keydown', e => {
  if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); evaluate(); }
  else if (e.key === 'ArrowUp' && !inp.value && history.length) {
    histPos = Math.max(0, histPos - 1); inp.value = history[histPos] || '';
    autoGrow(); e.preventDefault();
  } else if (e.key === 'ArrowDown' && !inp.value && history.length) {
    histPos = Math.min(history.length, histPos + 1);
    inp.value = history[histPos] || ''; autoGrow(); e.preventDefault();
  } else if (e.key === ' ' && e.ctrlKey) {
    e.preventDefault(); showComplete();
  } else if (e.key === 'Escape') {
    ac.style.display = 'none';
  }
});

function currentWord() {
  const pos = inp.selectionStart;
  const before = inp.value.slice(0, pos);
  const m = before.match(/[A-Za-z$]+$/);
  return m ? m[0] : '';
}
function showComplete() {
  const word = currentWord();
  if (!word || !NAMES.length) return;
  const matches = NAMES.filter(n => n.startsWith(word)).slice(0, 40);
  if (!matches.length) return;
  const rect = inp.getBoundingClientRect();
  ac.innerHTML = matches.map(n => '<div data-n="' + n + '">' + n + '</div>').join('');
  ac.style.left = rect.left + 'px';
  ac.style.top = (rect.bottom + 2) + 'px';
  ac.style.display = 'block';
  ac.querySelectorAll('div').forEach(d => {
    d.onclick = () => insertComplete(d.dataset.n);
  });
}
function insertComplete(name) {
  const pos = inp.selectionStart;
  const before = inp.value.slice(0, pos).replace(/[A-Za-z$]+$/, name);
  inp.value = before + inp.value.slice(inp.selectionStart);
  ac.style.display = 'none';
  inp.focus();
}
document.addEventListener('click', e => {
  if (!ac.contains(e.target)) ac.style.display = 'none';
});
document.getElementById('go').onclick = evaluate;
document.getElementById('reset').onclick = async () => {
  await fetch('/api/reset', {method: 'POST'});
  addCellHTML('<div class="msgs">kernel reset</div>');
};
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