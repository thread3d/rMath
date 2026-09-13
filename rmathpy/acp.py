"""Drive the harness through its ACP profile, for real tool lifecycle detail.

The headless profile streams only reasoning and a final answer, so tool activity
was invisible. The ACP profile (`dsh --profile acp`) speaks the standard Agent
Client Protocol over stdio — **newline-delimited JSON**, not Content-Length
framing — and publishes semantic updates: the agent's thoughts and messages, the
generic tool lifecycle (`tool_call` → `tool_call_update`, with title, kind and
status), context usage, and permission requests the client must answer.

One session is kept alive and reused, so a follow-up prompt does not pay the
startup cost and keeps its context. Stdlib only.
"""

import json
import os
import queue
import subprocess
import threading
import time

from . import harness, paths

PROCESS_TIMEOUT = 60

_proc = None
_session_id = None
_session_cwd = None
_reader = None
_updates = queue.Queue()
_pending = {}
_lock = threading.RLock()
_next_id = [0]
_last_error = ""
_backend_used = ""
_last_tool_title = "tool"
_message_parts = []          # assistant text, so the panel's answer box is filled


def _enabled():
    return os.environ.get("RMATH_NO_ACP", "") not in ("1", "true")


def available():
    return _enabled() and harness.find_dsh() is not None


def _overlay():
    """A per-run patch: route the model through the thinking proxy, pick the
    configured model, and disable the settings document (it outranks the
    environment, and the harness needs the endpoint to be the proxy)."""
    from . import ollama_proxy
    url = ollama_proxy.start() if ollama_proxy.running() else None
    if not url:
        url, _ = harness.endpoint()
    model = harness.harness_model() or "ornith-1.5:35b"
    body = ("# written by rMath for the ACP profile\n"
            "- id: settings\n  disabled: true\n"
            "- id: llm-deepseek\n  config:\n    baseURL: %s\n"
            "- id: acp\n  config:\n    provider: deepseek-official\n    model: %s\n" % (url, model))
    for path in (os.path.join(os.path.dirname(harness.keyfile()), "acp.patch.yml"),
                 os.path.join("/tmp", "rmath-acp.patch.yml")):
        try:
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w") as f:
                f.write(body)
            return path
        except OSError:
            continue
    return None


def _send(obj):
    with _lock:
        if _proc is None or _proc.poll() is not None:
            raise RuntimeError("the ACP process is not running")
        _proc.stdin.write(json.dumps(obj) + "\n")
        _proc.stdin.flush()


def _read_loop():
    global _last_error
    for line in _proc.stdout:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except Exception:
            continue
        method = msg.get("method")
        if method == "session/update":
            _updates.put(("update", (msg.get("params") or {}).get("update") or {}))
        elif method == "session/request_permission":
            # Trusted controller: allow the tool so the agent can proceed.
            options = (msg.get("params") or {}).get("options") or []
            pick = next((o for o in options if "allow" in json.dumps(o).lower()),
                        options[0] if options else None)
            try:
                if pick:
                    _send({"jsonrpc": "2.0", "id": msg.get("id"),
                           "result": {"outcome": {"outcome": "selected",
                                                  "optionId": pick.get("optionId")}}})
                else:
                    _send({"jsonrpc": "2.0", "id": msg.get("id"),
                           "result": {"outcome": {"outcome": "cancelled"}}})
            except Exception:
                pass
        elif method and "id" in msg:
            # We advertise no client filesystem support; refuse politely.
            try:
                _send({"jsonrpc": "2.0", "id": msg["id"],
                       "error": {"code": -32601, "message": "unsupported by this client"}})
            except Exception:
                pass
        elif "id" in msg:
            with _lock:
                slot = _pending.pop(msg["id"], None)
            if slot is not None:
                slot["reply"] = msg
                slot["event"].set()
        if msg.get("error"):
            _last_error = json.dumps(msg["error"])[:200]


def _request(method, params, timeout=PROCESS_TIMEOUT):
    with _lock:
        _next_id[0] += 1
        rid = _next_id[0]
        slot = {"event": threading.Event(), "reply": None}
        _pending[rid] = slot
    _send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params})
    if not slot["event"].wait(timeout):
        raise TimeoutError("%s timed out after %ss" % (method, timeout))
    reply = slot["reply"]
    if reply is None or reply.get("error"):
        raise RuntimeError("%s failed: %s" % (method, json.dumps((reply or {}).get("error"))[:200]))
    return reply.get("result")


def _spawn(cwd):
    """Start the ACP process and one session in `cwd`."""
    global _proc, _reader, _session_id, _session_cwd, _last_error
    overlay = _overlay()
    cmd = [harness.find_dsh(), "--profile", "acp"]
    if overlay:
        cmd += ["--patch", overlay]
    env = paths.child_env()
    env.update(harness.read_env())
    _proc = subprocess.Popen(cmd, env=env, cwd=cwd or harness.workspace_root(),
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, text=True, bufsize=1,
                             start_new_session=True)
    _reader = threading.Thread(target=_read_loop, daemon=True)
    _reader.start()
    _request("initialize", {"protocolVersion": 1,
                            "clientCapabilities": {"fs": {"readTextFile": False,
                                                          "writeTextFile": False}}})
    result = _request("session/new", {"cwd": cwd or harness.workspace_root(),
                                      "mcpServers": []})
    _session_id = result.get("sessionId")
    _session_cwd = cwd or harness.workspace_root()
    return _session_id


def session(cwd=None):
    """The live ACP session, starting one if needed."""
    global _session_id
    if _proc is None or _proc.poll() is not None or not _session_id:
        with _lock:
            _session_id = None
        return _spawn(cwd)
    if cwd and os.path.realpath(cwd) != os.path.realpath(_session_cwd or ""):
        # A session is bound to its workspace: start a fresh one only if the
        # harness process itself is gone; otherwise keep what we have.
        pass
    return _session_id


def current():
    if _proc is None or _proc.poll() is not None:
        return None
    return {"session": _session_id, "pid": _proc.pid, "cwd": _session_cwd}


def _drain_updates(on_line):
    """Forward queued updates as panel lines; returns the tool call titles seen."""
    seen = {}
    global _last_tool_title
    while True:
        try:
            kind, update = _updates.get_nowait()
        except queue.Empty:
            return seen
        if kind != "update":
            continue
        what = update.get("sessionUpdate")
        content = update.get("content")
        text = content.get("text", "") if isinstance(content, dict) else ""
        if what == "agent_thought_chunk" and text:
            on_line(text, "reasoning")
        elif what == "agent_message_chunk" and text:
            _message_parts.append(text)
            on_line(text, "answer")
        elif what == "tool_call":
            global _last_tool_title
            title = update.get("title") or update.get("kind") or "tool"
            _last_tool_title = title
            seen[update.get("toolCallId")] = title
            detail = update.get("rawInput")
            more = (" " + json.dumps(detail, separators=(",", ":"))[:90]) if detail else ""
            status = update.get("status") or "in_progress"
            on_line("\U0001f527 %s%s  [%s]" % (title, more, status), "tool")
        elif what == "tool_call_update":
            title = (update.get("title") or seen.get(update.get("toolCallId"))
                     or _last_tool_title or "tool")
            status = update.get("status") or "update"
            extra = ""
            for block in (update.get("content") or []):
                if isinstance(block, dict) and block.get("type") == "content":
                    inner = block.get("content") or {}
                    if isinstance(inner, dict) and inner.get("text"):
                        extra = " " + " ".join(inner["text"].split())[:80]
            on_line("\u2713 %s [%s]%s" % (title, status, extra), "tool")
        elif what == "usage_update":
            used, size = update.get("used"), update.get("size")
            if used is not None:
                on_line("\u2022 context %s/%s tokens" % (used, size), "meta")


def run_task(task, timeout=1800, on_line=None, cwd=None):
    """Send one prompt and stream its lifecycle. Returns a result dict."""
    global _backend_used, _last_error
    _last_error = ""
    handler = on_line or (lambda text, kind: None)
    try:
        sid = session(cwd)
    except Exception as e:
        return {"status": "error", "error": "ACP session failed: %s" % e,
                "answer": "", "reasoning": "", "via": "acp"}
    del _message_parts[:]
    _drain_updates(lambda t, k: None)          # clear anything stale
    t0 = time.time()
    done = threading.Event()
    result = {}

    def prompt_call():
        try:
            result["reply"] = _request("session/prompt",
                                       {"sessionId": sid,
                                        "prompt": [{"type": "text", "text": task}]},
                                       timeout=timeout)
        except Exception as e:
            result["error"] = str(e)
        finally:
            done.set()

    thread = threading.Thread(target=prompt_call, daemon=True)
    thread.start()
    deadline = time.time() + timeout
    answers = []
    while not done.is_set() and time.time() < deadline:
        seen = _drain_updates(handler)
        if done.wait(0.4):
            break
    _drain_updates(handler)
    if result.get("error"):
        _last_error = result["error"]
        return {"status": "error", "error": result["error"],
                "answer": "".join(_message_parts).strip(),
                "reasoning": "", "via": "acp", "seconds": round(time.time() - t0, 1)}
    stop_reason = (result.get("reply") or {}).get("stopReason")
    _backend_used = "acp"
    return {"status": "ok", "answer": "".join(_message_parts).strip(), "reasoning": "",
            "via": "acp", "stop_reason": stop_reason,
            "seconds": round(time.time() - t0, 1), "session": sid,
            "endpoint": harness.endpoint()[0],
            "endpoint_source": harness.endpoint()[1]}


def cancel():
    """Ask the running prompt to stop (the Stop button)."""
    if _proc is None or _proc.poll() is not None or not _session_id:
        return {"status": "ok", "cancelled": False, "note": "no ACP session"}
    try:
        _send({"jsonrpc": "2.0", "method": "session/cancel",
               "params": {"sessionId": _session_id}})
        return {"status": "ok", "cancelled": True, "session": _session_id}
    except Exception as e:
        return {"status": "error", "error": str(e)}


def stop():
    global _proc, _session_id, _reader
    if _proc is not None:
        try:
            _proc.terminate()
        except Exception:
            pass
    _proc = None
    _session_id = None
    _reader = None
