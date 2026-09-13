"""Rename Ollama's reasoning deltas so the harness can show the model's thinking.

The harness's DeepSeek adapter reads the model's thinking from the SSE field
`reasoning_content` (`dsh-llm-deepseek/lib/index.js`, `delta?.reasoning_content`),
while Ollama streams it as `reasoning`. With a local model that difference meant
the harness showed *nothing* until the final answer — no thinking, no progress.

This proxy sits between the two: it forwards the OpenAI-compatible chat
completions endpoint to Ollama, rewriting `reasoning` to `reasoning_content` in
the streamed chunks (and in non-streaming replies), leaving everything else
untouched. Stdlib only, no dependencies.
"""

import json
import os
import threading
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

UPSTREAM = os.environ.get("RMATH_OLLAMA_URL", "http://localhost:11434").rstrip("/")
DEFAULT_PORT = int(os.environ.get("RMATH_PROXY_PORT", "11435"))

STATE = {"requests": 0, "rewritten": 0, "reasoning_chars": 0, "last_error": ""}
_lock = threading.Lock()
_server = None
_thread = None


def rewrite_chunk(line):
    """Rewrite one SSE line, or a whole non-streaming JSON body."""
    stripped = line.strip()
    prefix = b""
    if stripped.startswith(b"data:"):
        prefix = b"data: "
        payload = stripped[5:].strip()
    else:
        payload = stripped
    if payload in (b"", b"[DONE]"):
        return line
    try:
        obj = json.loads(payload)
    except Exception:
        return line
    changed = False
    chars = 0
    for choice in obj.get("choices") or []:
        for holder in (choice.get("delta"), choice.get("message")):
            if isinstance(holder, dict) and "reasoning" in holder \
                    and "reasoning_content" not in holder:
                text = holder.pop("reasoning")
                holder["reasoning_content"] = text
                chars += len(text) if isinstance(text, str) else 0
                changed = True
    if not changed:
        return line
    with _lock:
        STATE["rewritten"] += 1
        STATE["reasoning_chars"] += chars
    return prefix + json.dumps(obj, separators=(",", ":")).encode() + b"\n"


class Handler(BaseHTTPRequestHandler):
    # HTTP/1.1 with chunked encoding, so the client sees each reasoning chunk as
    # it arrives instead of everything at connection close.
    protocol_version = "HTTP/1.1"
    server_version = "rmath-thinking-proxy"

    def log_message(self, *args):
        pass                                # quiet

    def _forward(self, method):
        path = self.path
        if path == "/__stats":
            with _lock:
                body = json.dumps(dict(STATE, upstream=UPSTREAM)).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length) if length else None
        with _lock:
            STATE["requests"] += 1
        req = urllib.request.Request(UPSTREAM + path, data=body, method=method)
        for key in ("Content-Type", "Accept", "Authorization"):
            if self.headers.get(key):
                req.add_header(key, self.headers[key])
        try:
            with urllib.request.urlopen(req, timeout=1800) as upstream:
                self.send_response(upstream.status)
                ctype = upstream.headers.get("Content-Type", "application/json")
                self.send_header("Content-Type", ctype)
                streaming = "event-stream" in ctype
                if not streaming:
                    data = upstream.read()
                    data = rewrite_chunk(data)
                    self.send_header("Content-Length", str(len(data)))
                    self.end_headers()
                    self.wfile.write(data)
                    return
                self.send_header("Transfer-Encoding", "chunked")
                self.send_header("Cache-Control", "no-store")
                self.end_headers()
                for raw in upstream:
                    out = raw if not raw.strip() else rewrite_chunk(raw)
                    self.wfile.write(b"%x\r\n" % len(out) + out + b"\r\n")
                    self.wfile.flush()
                self.wfile.write(b"0\r\n\r\n")
                self.wfile.flush()
        except urllib.error.HTTPError as e:
            detail = e.read()[:400]
            with _lock:
                STATE["last_error"] = "HTTP %s: %s" % (e.code, detail[:200])
            self.send_response(e.code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(detail)))
            self.end_headers()
            self.wfile.write(detail)
        except (BrokenPipeError, ConnectionResetError):
            return                          # the client hung up: normal
        except Exception as e:
            with _lock:
                STATE["last_error"] = str(e)
            try:
                msg = json.dumps({"error": {"message": "thinking proxy: %s" % e}}).encode()
                self.send_response(502)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(msg)))
                self.end_headers()
                self.wfile.write(msg)
            except Exception:
                pass

    def handle_one_request(self):
        try:
            BaseHTTPRequestHandler.handle_one_request(self)
        except (BrokenPipeError, ConnectionResetError):
            self.close_connection = True

    def do_POST(self):
        self._forward("POST")

    def do_GET(self):
        self._forward("GET")


class QuietServer(ThreadingHTTPServer):
    """A handler that went away is routine; do not print a traceback for it."""

    daemon_threads = True

    def handle_error(self, request, client_address):
        import sys
        exc = sys.exc_info()[1]
        if isinstance(exc, (BrokenPipeError, ConnectionResetError)):
            return
        super().handle_error(request, client_address)


def start(port=DEFAULT_PORT, timeout=5.0):
    """Start the proxy once; return its base URL, or None if it cannot run."""
    global _server, _thread
    if _server is not None:
        return "http://127.0.0.1:%d/v1" % _server.server_address[1]
    try:
        _server = QuietServer(("127.0.0.1", port), Handler)
    except OSError:
        return None
    _thread = threading.Thread(target=_server.serve_forever, daemon=True)
    _thread.start()
    return "http://127.0.0.1:%d/v1" % _server.server_address[1]


def running():
    return _server is not None


def stats():
    with _lock:
        return dict(STATE)


def stop():
    global _server, _thread
    if _server is not None:
        _server.shutdown()
        _server.server_close()
        _server = None
        _thread = None
