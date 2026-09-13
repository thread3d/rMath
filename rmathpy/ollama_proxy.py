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

STATE = {"requests": 0, "rewritten": 0, "reasoning_chars": 0, "last_error": "",
         "tool_calls": 0, "tool_results": 0}
# Tool activity seen on the wire, for the panel to drain while a run is in flight:
# the proxy sits between the harness and the model, so it sees both the calls the
# model asks for and the results the harness sends back.
EVENTS = []
_events_lock = threading.Lock()
_partial_calls = {}


def emit_event(text):
    with _events_lock:
        EVENTS.append(text)
        del EVENTS[:-200]


def drain_events():
    with _events_lock:
        out = list(EVENTS)
        EVENTS.clear()
    return out


def _short(value, limit=120):
    text = value if isinstance(value, str) else json.dumps(value, separators=(",", ":"))
    text = " ".join(str(text).split())
    return text[:limit] + ("\u2026" if len(text) > limit else "")


DEBUG = os.environ.get("RMATH_PROXY_DEBUG", "") not in ("", "0", "false")


def _debug(line):
    if not DEBUG:
        return
    try:
        with open("/tmp/rmath-proxy-debug.log", "a") as f:
            f.write(line + "\n")
    except OSError:
        pass


def note_request(obj):
    """Tool results come back to the model as `tool` role messages."""
    if DEBUG:
        roles = [m.get("role") for m in (obj.get("messages") or []) if isinstance(m, dict)]
        _debug("REQUEST roles=%s tools=%s tool_calls=%s" % (
            roles, len(obj.get("tools") or []),
            sum(1 for m in (obj.get("messages") or [])
                if isinstance(m, dict) and m.get("tool_calls"))))
    for msg in obj.get("messages") or []:
        if not isinstance(msg, dict) or msg.get("role") != "tool":
            continue
        content = msg.get("content")
        if isinstance(content, list):
            content = " ".join(c.get("text", "") for c in content if isinstance(c, dict))
        with _lock:
            STATE["tool_results"] += 1
        emit_event("\u2713 result: " + _short(content or "(empty)"))


def note_tool_calls(obj, final=False):
    """Tool calls appear in a reply's `message.tool_calls` or streamed deltas."""
    if DEBUG and isinstance(obj, dict):
        for choice in obj.get("choices") or []:
            for holder in (choice.get("delta"), choice.get("message")):
                if isinstance(holder, dict):
                    keys = sorted(holder.keys())
                    if "tool_calls" in holder or holder.get("finish_reason"):
                        _debug("RESPONSE holder keys=%s tool_calls=%s" % (keys, bool(holder.get("tool_calls"))))
                    if holder.get("content") and not holder.get("tool_calls"):
                        _debug("RESPONSE text=%s" % str(holder.get("content"))[:160].replace("\n", " "))
    for choice in obj.get("choices") or []:
        for holder in (choice.get("delta"), choice.get("message")):
            if not isinstance(holder, dict):
                continue
            for call in holder.get("tool_calls") or []:
                idx = call.get("index", 0)
                fn = call.get("function") or {}
                slot = _partial_calls.setdefault(idx, {"name": "", "arguments": ""})
                if fn.get("name"):
                    slot["name"] = fn["name"]
                if fn.get("arguments"):
                    slot["arguments"] += fn["arguments"]
                if final or fn.get("name"):
                    with _lock:
                        STATE["tool_calls"] += 1
                    emit_event("\U0001f527 tool: %s %s" % (slot["name"] or "?",
                                                            _short(slot["arguments"], 90)))
                    _partial_calls.pop(idx, None)
        if final:
            _partial_calls.clear()
_lock = threading.Lock()
_server = None
_thread = None
_external = None          # base URL of a proxy already running in another process


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
    note_tool_calls(obj, final=bool(obj.get("__final")) if isinstance(obj, dict) else False)
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
        if path == "/__events":
            body = json.dumps(drain_events()).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
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
        if body and path.endswith("chat/completions"):
            try:
                note_request(json.loads(body))
            except Exception:
                pass
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
                    try:
                        note_tool_calls(json.loads(data), final=True)
                    except Exception:
                        pass
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
                    if raw.strip().startswith(b"data:"):
                        payload = raw.strip()[5:].strip()
                        if payload not in (b"", b"[DONE]"):
                            try:
                                note_tool_calls(json.loads(payload))
                            except Exception:
                                pass
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


def _probe_existing(port):
    """Is a thinking proxy already listening on this port? """
    try:
        with urllib.request.urlopen("http://127.0.0.1:%d/__stats" % port, timeout=2) as r:
            data = json.load(r)
        if "requests" in data:
            return "http://127.0.0.1:%d/v1" % port
    except Exception:
        pass
    return None


def start(port=DEFAULT_PORT, timeout=5.0):
    """Start the proxy once; return its base URL, or None if it cannot run.

    A proxy may already be running in another process (the app's notebook
    server): the fixed port then cannot be bound, so attach to that one instead
    of reporting failure — otherwise a second client silently takes the direct
    route and loses the tool/thinking events.
    """
    global _server, _thread, _external
    if _server is not None:
        return "http://127.0.0.1:%d/v1" % _server.server_address[1]
    if _external:
        return _external
    try:
        _server = QuietServer(("127.0.0.1", port), Handler)
    except OSError:
        _external = _probe_existing(port)
        return _external
    _thread = threading.Thread(target=_server.serve_forever, daemon=True)
    _thread.start()
    return "http://127.0.0.1:%d/v1" % _server.server_address[1]


def running():
    return _server is not None or bool(_external)


def _external_json(path, fallback):
    base = (_external or "").replace("/v1", "")
    if not base:
        return fallback
    try:
        with urllib.request.urlopen(base + path, timeout=3) as r:
            return json.load(r)
    except Exception:
        return fallback


def stats():
    if _server is None and _external:
        return _external_json("/__stats", {})
    with _lock:
        return dict(STATE)


def drain_events_remote():
    """Events from a proxy in another process."""
    return _external_json("/__events", [])


def stop():
    global _server, _thread, _external
    if _server is not None:
        _server.shutdown()
        _server.server_close()
        _server = None
        _thread = None
        _external = None
