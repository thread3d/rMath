"""Drive the DeepSeek Harness (`dsh`) from the rMath front end.

The harness is addressed through its headless profile:

    dsh --profile headless "<task>"

which runs one fresh session, streams its reasoning to stderr, prints the final
answer to stdout and exits. The agent's workspace root is the directory the
command runs in, so rMath points it at the project: the agent can read the
repository, edit files and run the build.
"""

import json
import os
import shutil
import signal
import subprocess
import tempfile
import threading
import time
import urllib.request


def find_dsh():
    """Locate the `dsh` executable."""
    env = os.environ.get("DSH_BIN")
    if env and os.path.isfile(env):
        return env
    from .paths import find_tool
    return find_tool("dsh")


def workspace_root():
    """The directory the harness runs in (its workspace root): the same
    repository the front end came from, even when running inside the app
    bundle."""
    from .paths import workspace_root as resolve
    return resolve()


def profile():
    return os.environ.get("RMATH_DSH_PROFILE", "headless")


# Ollama serves an OpenAI-compatible API at /v1; the adapter POSTs to
# <baseURL>/chat/completions, so the suffix is required.
OLLAMA_DEFAULT = "http://localhost:11434/v1"
DEEPSEEK_DEFAULT = "https://api.deepseek.com"


def ollama_available(timeout=1.5):
    """True when a local Ollama answers on its default port."""
    try:
        with urllib.request.urlopen("http://localhost:11434/api/version", timeout=timeout) as r:
            return r.status == 200
    except Exception:
        return False


def ollama_models(timeout=4):
    """Model ids the local Ollama offers (OpenAI-compatible listing)."""
    try:
        req = urllib.request.Request("http://localhost:11434/v1/models")
        with urllib.request.urlopen(req, timeout=timeout) as r:
            data = json.load(r)
        return [m.get("id", "") for m in data.get("data", []) if m.get("id")]
    except Exception:
        return []


def endpoint():
    """The endpoint the harness should use, and where that choice came from."""
    env = os.environ.get("DEEPSEEK_BASE_URL", "").strip()
    if env:
        return env, "environment"
    # A local Ollama streams the model's thinking as `reasoning`, which the
    # harness's adapter does not read; the proxy renames it so the panel can show
    # the thinking as it happens.
    if os.environ.get("RMATH_NO_THINKING_PROXY", "") in ("1", "true"):
        pass
    else:
        from . import ollama_proxy
        if ollama_proxy.running():
            return ollama_proxy.start() or OLLAMA_DEFAULT, "local ollama (thinking proxy)"
    for line in _keyfile_lines():
        if line.startswith("DEEPSEEK_BASE_URL="):
            return line.split("=", 1)[1].strip(), "keyfile"
    if ollama_available():
        return OLLAMA_DEFAULT, "local ollama"
    return DEEPSEEK_DEFAULT, "default"


def settings_path():
    home = os.environ.get("DSH_HOME") or os.path.expanduser("~/.dsh")
    return os.environ.get("RMATH_DSH_SETTINGS") or os.path.join(home, "settings.yaml")


def harness_model():
    """The default model the harness starts agents on, read from its settings
    document (a tiny parse of the `agent-default-model:` section, so no YAML
    dependency is needed)."""
    try:
        with open(settings_path()) as f:
            lines = f.readlines()
    except OSError:
        return ""
    section = None
    for raw in lines:
        line = raw.split("#", 1)[0].rstrip()
        if not line.strip():
            continue
        indent = len(line) - len(line.lstrip())
        body = line.strip()
        if indent == 0:
            section = body[:-1] if body.endswith(":") else None
            continue
        if section == "agent-default-model":
            name, _, val = body.partition(":")
            if name.strip() == "model":
                return val.strip().strip("\"'")
    return ""


def _keyfile_lines():
    try:
        with open(keyfile()) as f:
            return [l.strip() for l in f if l.strip() and not l.startswith("#")]
    except OSError:
        return []


def keyfile():
    """Where a pasted provider key is kept (never inside the repository)."""
    env = os.environ.get("RMATH_HARNESS_KEYFILE")
    if env:
        return env
    base = os.path.expanduser("~/Library/Application Support/rMath")
    if not os.path.isdir(base):
        base = os.path.join(os.path.expanduser("~"), ".rmath")
    return os.path.join(base, "harness.env")


def read_key():
    """The provider key for headless runs: environment, keyfile, then a
    placeholder for a local endpoint (Ollama ignores the value)."""
    env = os.environ.get("DEEPSEEK_API_KEY", "").strip()
    if env:
        return env
    # Only an explicit DEEPSEEK_API_KEY= line counts; treating arbitrary file
    # content as a secret would send whatever happens to be there.
    for line in _keyfile_lines():
        if line.startswith("DEEPSEEK_API_KEY="):
            return line.split("=", 1)[1].strip()
    url, _ = endpoint()
    if "localhost" in url or "127.0.0.1" in url:
        return "ollama"  # any non-empty value satisfies the credentials seam
    return ""


# Environment names the keyfile may carry through to the harness. The adapter
# resolves the endpoint from $DEEPSEEK_BASE_URL (default https://api.deepseek.com)
# and the key through its credentials seam, then the environment.
PASSTHROUGH = ("DEEPSEEK_API_KEY", "DEEPSEEK_BASE_URL", "RMATH_DSH_PROFILE")


def read_env():
    """Environment additions for a headless run (key, endpoint, profile)."""
    out = {}
    for line in _keyfile_lines():
        if "=" not in line:
            continue
        name, _, val = line.partition("=")
        name, val = name.strip(), val.strip()
        if name in PASSTHROUGH and val:
            out[name] = val
    key = read_key()
    if key:
        out["DEEPSEEK_API_KEY"] = key
    url, _ = endpoint()
    if url:
        out["DEEPSEEK_BASE_URL"] = url
    return out


def store_key(key, base_url=""):
    """Persist the key (and optional endpoint) with 0600 permissions."""
    path = keyfile()
    os.makedirs(os.path.dirname(path), exist_ok=True)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as f:
        f.write("DEEPSEEK_API_KEY=%s\n" % key.strip())
        if base_url.strip():
            f.write("DEEPSEEK_BASE_URL=%s\n" % base_url.strip())
    return path


def available():
    return find_dsh() is not None


_CURRENT = {"proc": None, "started": None, "task": "", "cancelled": False}


def current():
    """The run in flight, if any: its task, start time and whether it can stop."""
    proc = _CURRENT.get("proc")
    if proc is None or proc.poll() is not None:
        return None
    return {"task": _CURRENT["task"][:200], "started": _CURRENT["started"],
            "pid": proc.pid}


def cancel_current():
    """Terminate the running harness task (the Stop button).

    A headless run can take minutes and cannot be interrupted through the stream,
    so the child process is tracked here and killed on request."""
    proc = _CURRENT.get("proc")
    if proc is None or proc.poll() is not None:
        return {"status": "ok", "cancelled": False,
                "note": "no harness task is running"}
    _CURRENT["cancelled"] = True
    try:
        # Signal the whole group: `dsh` is a node process that may have children.
        try:
            os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
        except Exception:
            proc.terminate()
        for _ in range(30):
            if proc.poll() is not None:
                break
            time.sleep(0.1)
        if proc.poll() is None:
            try:
                os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
            except Exception:
                proc.kill()
    except Exception as e:
        return {"status": "error", "error": str(e)}
    return {"status": "ok", "cancelled": True, "pid": proc.pid}


def proxy_patch():
    """A per-run overlay that routes the model through the thinking proxy.

    The adapter resolves its endpoint as `config.baseURL ?? $DEEPSEEK_BASE_URL`,
    so the user settings document wins over the environment; disabling that row
    for this invocation lets the overlay supply the endpoint instead. Nothing
    global is modified, so a terminal `dsh` run is unaffected.
    """
    from . import ollama_proxy
    if not ollama_proxy.running():
        return None
    url = ollama_proxy.start()
    if not url:
        return None
    model = harness_model() or "deepseek-flash"
    body = ("# written by rMath: show the model's thinking while it works\n"
            "- id: settings\n"
            "  disabled: true\n"
            "- id: llm-deepseek\n"
            "  config:\n"
            "    baseURL: %s\n"
            "- id: agent-default-model\n"
            "  config:\n"
            "    provider: deepseek-official\n"
            "    model: %s\n" % (url, model))
    for path in (os.path.join(os.path.dirname(keyfile()), "thinking.patch.yml"),
                 os.path.join(tempfile.gettempdir(), "rmath-thinking.patch.yml")):
        try:
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w") as f:
                f.write(body)
            return path
        except OSError:
            continue
    return None


def run_task(task, timeout=1800, on_line=None, cwd=None):
    """Run one headless task.

    `on_line(text)` is called for each streamed line (reasoning and answer).
    Returns a dict: status, answer, reasoning, exit, error.
    """
    dsh = find_dsh()
    if dsh is None:
        return {
            "status": "error",
            "error": "the DeepSeek Harness (`dsh`) was not found; set DSH_BIN or install it",
            "answer": "",
            "reasoning": "",
            "exit": None,
        }
    root = cwd or workspace_root()
    patch = proxy_patch()
    cmd = [dsh, "--profile", profile()]
    if patch:
        cmd += ["--patch", patch]
    cmd.append(task)
    # A minimal PATH cannot run a node script: `dsh` would die with
    # "env: node: No such file or directory", which is exactly what a bundled
    # app showed.
    from .paths import child_env
    env = child_env()
    env.update(read_env())
    try:
        proc = subprocess.Popen(
            cmd,
            cwd=root,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
            start_new_session=True,     # its own process group, so Stop stops all of it
        )
    except OSError as e:
        return {"status": "error", "error": str(e), "answer": "", "reasoning": "", "exit": None}

    try:                                  # bookkeeping must never break a run
        _CURRENT["proc"] = proc
        _CURRENT["started"] = time.time()
        _CURRENT["task"] = task
        _CURRENT["cancelled"] = False
    except Exception:
        pass
    out_lines, err_lines = [], []

    def pump(stream, sink, kind):
        # The headless contract: reasoning streams on stderr, the final answer
        # is flushed to stdout. Keep them apart so the panel can show the
        # reasoning quietly and typeset the answer.
        try:
            for line in stream:
                sink.append(line)
                if on_line is not None:
                    on_line(line.rstrip("\n"), kind)
        except Exception:
            pass

    t_out = threading.Thread(target=pump, args=(proc.stdout, out_lines, "answer"), daemon=True)
    t_err = threading.Thread(target=pump, args=(proc.stderr, err_lines, "reasoning"), daemon=True)
    t_out.start()
    t_err.start()
    try:
        code = proc.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        proc.kill()
        code = -1
        if on_line is not None:
            on_line("[rMath] harness run timed out after %ds" % timeout)
    t_out.join(timeout=3)
    t_err.join(timeout=3)
    cancelled = bool(_CURRENT.get("cancelled"))
    _CURRENT["proc"] = None
    _CURRENT["cancelled"] = False

    url, source = endpoint()
    return {
        "status": "ok" if code == 0 else "error",
        "cancelled": bool(cancelled),
        "answer": "".join(out_lines).strip(),
        "reasoning": "".join(err_lines).strip(),
        "exit": code,
        "error": "" if code == 0 else "harness exited with code %s" % code,
        "workspace": root,
        "endpoint": url,
        "endpoint_source": source,
        "thinking_patch": patch,
    }


def cell_task(expression, result="", messages="", instruction=""):
    """Build a task that asks the harness about one evaluated cell."""
    parts = [
        "You are helping inside the rMath project (a Mathematica-like system: Rust",
        "kernel in rmath/, Python notebook front end in rmathpy/, native app in",
        "apps/native/). Use the repository to answer.",
        "",
        "A notebook cell was evaluated:",
        "  input:    %s" % (expression or "(empty)"),
    ]
    if result:
        parts.append("  output:   %s" % result)
    if messages:
        parts.append("  messages: %s" % messages)
    parts.append("")
    parts.append(instruction or "Explain what happened, and if the result or any "
                                "message is wrong, find the cause in the code, fix "
                                "it, rebuild and verify.")
    parts.append("")
    parts.append("Be concise. If you change code, list the files and the commands "
                 "you ran to verify.")
    return "\n".join(parts)


def fix_task(report_path, extra=""):
    """Build a task that asks the harness to fix the bug a report describes."""
    return "\n".join([
        "A bug was reported in the rMath project. The report (with a screenshot",
        "path, the notebook transcript, versions and the server log) is at:",
        "  %s" % report_path,
        "",
        "Read the report, then find and fix the underlying bug in this repository.",
        "Use the tools available to inspect code, edit files and run commands.",
        "After fixing: rebuild (make release and make app if needed), run the tests",
        "(make test) and the UI test (make uitest), and make sure they pass.",
        "",
        "Summarise: the root cause, the files changed, and the commands you ran",
        "with their outcome." + (("\n\n" + extra) if extra else ""),
    ])
