"""Drive the DeepSeek Harness (`dsh`) from the rMath front end.

The harness is addressed through its headless profile:

    dsh --profile headless "<task>"

which runs one fresh session, streams its reasoning to stderr, prints the final
answer to stdout and exits. The agent's workspace root is the directory the
command runs in, so rMath points it at the project: the agent can read the
repository, edit files and run the build.
"""

import os
import shutil
import subprocess
import threading


def find_dsh():
    """Locate the `dsh` executable."""
    env = os.environ.get("DSH_BIN")
    if env and os.path.isfile(env):
        return env
    found = shutil.which("dsh")
    if found:
        return found
    for cand in (
        os.path.expanduser("~/.npm-packages/bin/dsh"),
        os.path.expanduser("~/.local/bin/dsh"),
        "/usr/local/bin/dsh",
        "/opt/homebrew/bin/dsh",
    ):
        if os.path.isfile(cand):
            return cand
    return None


def workspace_root():
    """The directory the harness runs in (its workspace root)."""
    env = os.environ.get("RMATH_HARNESS_ROOT")
    if env and os.path.isdir(env):
        return env
    # rmathpy/ lives in the project root.
    return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def profile():
    return os.environ.get("RMATH_DSH_PROFILE", "headless")


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
    """The provider key for headless runs: environment first, then the keyfile."""
    env = os.environ.get("DEEPSEEK_API_KEY", "").strip()
    if env:
        return env
    try:
        with open(keyfile()) as f:
            for line in f:
                line = line.strip()
                if line.startswith("DEEPSEEK_API_KEY="):
                    return line.split("=", 1)[1].strip()
                if line and "=" not in line:
                    return line  # a bare key
    except OSError:
        pass
    return ""


# Environment names the keyfile may carry through to the harness. The adapter
# resolves the endpoint from $DEEPSEEK_BASE_URL (default https://api.deepseek.com)
# and the key through its credentials seam, then the environment.
PASSTHROUGH = ("DEEPSEEK_API_KEY", "DEEPSEEK_BASE_URL", "RMATH_DSH_PROFILE")


def read_env():
    """Environment additions for a headless run, from the keyfile."""
    out = {}
    key = read_key()
    if key:
        out["DEEPSEEK_API_KEY"] = key
    try:
        with open(keyfile()) as f:
            for line in f:
                line = line.strip()
                if not line or line.startswith("#"):
                    continue
                name, _, val = line.partition("=")
                name, val = name.strip(), val.strip()
                if name in PASSTHROUGH and val:
                    out[name] = val
    except OSError:
        pass
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
    cmd = [dsh, "--profile", profile(), task]
    env = dict(os.environ)
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
        )
    except OSError as e:
        return {"status": "error", "error": str(e), "answer": "", "reasoning": "", "exit": None}

    out_lines, err_lines = [], []

    def pump(stream, sink, prefix):
        try:
            for line in stream:
                sink.append(line)
                if on_line is not None:
                    on_line(prefix + line.rstrip("\n"))
        except Exception:
            pass

    t_out = threading.Thread(target=pump, args=(proc.stdout, out_lines, ""), daemon=True)
    t_err = threading.Thread(target=pump, args=(proc.stderr, err_lines, ""), daemon=True)
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

    return {
        "status": "ok" if code == 0 else "error",
        "answer": "".join(out_lines).strip(),
        "reasoning": "".join(err_lines).strip(),
        "exit": code,
        "error": "" if code == 0 else "harness exited with code %s" % code,
        "workspace": root,
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
