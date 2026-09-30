"""Lean 4 bridge for rMath.

Two jobs:

* check a Lean snippet (used by the Lean panel), returning structured
  diagnostics, and
* verify an rMath claim: translate the rMath expression to a Lean statement and
  let Mathlib's automation prove it.

The rMath side is translated by parsing it as a Python expression (the syntax
overlaps almost completely once `[...]` becomes `(...)` and `^` becomes `**`),
which avoids writing yet another parser.
"""

import ast
import atexit
import copy
import json
import os
import re
import subprocess
import threading
import time

# Functions whose rMath name maps to a Lean name. Names not listed are treated
# as free variables (or, for known constants, as Lean constants).
FUNCS = {
    "Sqrt": "Real.sqrt",
    "Exp": "Real.exp",
    "Log": "Real.log",
    "Sin": "Real.sin",
    "Cos": "Real.cos",
    "Tan": "Real.tan",
    "Sinh": "Real.sinh",
    "Cosh": "Real.cosh",
    "Tanh": "Real.tanh",
    "ArcSin": "Real.arcsin",
    "ArcCos": "Real.arccos",
    "ArcTan": "Real.arctan",
    "Abs": "abs",
    "Max": "max",
    "Min": "min",
    "Floor": "Int.floor",
    "Ceiling": "Int.ceil",
}
CONSTS = {
    "Pi": "Real.pi",
    "E": "Real.exp 1",
    "Infinity": "(0 : ℝ)",  # not translated faithfully; refused below
}
# Silence the linters that flag the tactic alternatives that did not run: the
# chain is deliberately a set of fallbacks.
QUIET = ("set_option linter.unreachableTactic false\n"
         "set_option linter.unusedTactic false\n"
         "set_option linter.unnecessarySimpa false\n"
         "set_option autoImplicit false\n\n")

# Functions that are real-valued, forcing the ℝ interpretation.
REAL_FUNCS = {v for k, v in FUNCS.items() if v.startswith("Real.")}

# Used as `solve | ...`: only an alternative that *closes* the goal counts.
# `first` would stop at a tactic that merely simplified, leaving the goal open.
# Ordered cheapest/most-likely first; `solve |` only accepts a tactic that
# *closes* the goal, so the search engines go last. `rmath_radical` is defined in
# `lean/RMathLemmas.lean`, which is inlined into every generated file.
TACTICS = ("rmath_radical | norm_num [Real.sq_sqrt] | norm_num | ring | ring_nf | "
           "field_simp | norm_cast | positivity | linarith | nlinarith | gcongr | "
           "omega | simp | simp_all | decide | rfl | aesop | grind")

# Root substitution additionally knows how to use `Real.sq_sqrt`, which closes
# real-radical roots like (√2)² − 2 = 0 that plain `norm_num` leaves open.
SOLVE_TACTICS = ("rmath_radical | norm_num [Real.sq_sqrt] | norm_num | ring | ring_nf | "
                 "field_simp | norm_cast | nlinarith | linarith | positivity | gcongr | "
                 "omega | simp | simp_all | decide | rfl | aesop | grind")


def project_dir():
    """Directory of the Lean project (holds lakefile.* and lean-toolchain)."""
    from .paths import lean_project
    return lean_project()


def elan_bin():
    return os.environ.get("ELAN_BIN") or os.path.expanduser("~/.elan/bin")


def lake():
    import shutil

    found = shutil.which("lake")
    if found:
        return found
    cand = os.path.join(elan_bin(), "lake")
    return cand if os.path.isfile(cand) else None


def lean():
    import shutil

    found = shutil.which("lean")
    if found:
        return found
    cand = os.path.join(elan_bin(), "lean")
    return cand if os.path.isfile(cand) else None


def available():
    return lake() is not None


def toolchain():
    try:
        with open(os.path.join(project_dir(), "lean-toolchain")) as f:
            return f.read().strip()
    except OSError:
        return ""



# ------------------------------------------------------- persistent LSP server
#
# One `lean --server` per project, spoken to over LSP. Mathlib is loaded once,
# so each later check costs elaboration only instead of a full import.

class LeanServer:
    """A long-lived `lake env lean --server` conversation."""

    def __init__(self, project, timeout=600):
        self.project = project
        # One persistent session file (gitignored), changed in place so Mathlib
        # is imported once.
        self.uri = "file://" + os.path.join(project, "RMathCheck.lean")
        self.path = os.path.join(project, "RMathCheck.lean")
        self.timeout = timeout
        self.proc = None
        self.version = 0
        self.opened = False
        self.seq = 0
        self.lock = threading.RLock()
        self.check_lock = threading.Lock()
        self.msgs = []
        self.cond = threading.Condition(self.lock)
        self.next_id = 1
        self.replies = {}
        self.diag_seq = 0
        self.done_seq = 0     # $/lean/fileProgress with no work left
        self.trace = []       # (method, summary) for debugging
        self.stderr = ""
        self.logs = []        # window/logMessage + showMessage text (e.g. #print axioms)
        self.trusted = None   # calibration result; None = not run yet

    # ---- plumbing
    def _env(self):
        env = dict(os.environ)
        env["PATH"] = elan_bin() + ":" + env.get("PATH", "")
        return env

    def start(self):
        lk = lake()
        if lk is None:
            raise RuntimeError("lake not found")
        self.proc = subprocess.Popen([lk, "env", "lean", "--server"], cwd=self.project,
                                     env=self._env(), stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=0)
        threading.Thread(target=self._read_loop, daemon=True).start()
        threading.Thread(target=self._err_loop, daemon=True).start()
        self._request("initialize", {"processId": None,
                                     "rootUri": "file://" + self.project,
                                     "capabilities": {}}, timeout=300)
        self._notify("initialized", {})
        return self

    def _read_loop(self):
        stream = self.proc.stdout
        while True:
            headers = b""
            while not headers.endswith(b"\r\n\r\n"):
                ch = stream.read(1)
                if not ch:
                    return
                headers += ch
            length = 0
            for line in headers.decode(errors="replace").split("\r\n"):
                if line.lower().startswith("content-length"):
                    length = int(line.split(":")[1])
            body = b""
            while len(body) < length:
                chunk = stream.read(length - len(body))
                if not chunk:
                    return
                body += chunk
            try:
                msg = json.loads(body)
            except Exception:
                continue
            method = msg.get("method")
            with self.cond:
                if "id" in msg and ("result" in msg or "error" in msg):
                    self.replies[msg["id"]] = msg
                if method == "textDocument/publishDiagnostics":
                    self.msgs.append(msg)
                    self.diag_seq += 1
                elif method in ("window/logMessage", "window/showMessage"):
                    text = (msg.get("params") or {}).get("message", "")
                    if text:
                        self.logs.append(text)
                        del self.logs[:-200]
                elif method == "$/lean/fileProgress":
                    if not (msg.get("params") or {}).get("processing"):
                        self.done_seq += 1
                if method:
                    self.trace.append((method, json.dumps(msg.get("params", {}))[:120]))
                    del self.trace[:-40]
                self.cond.notify_all()

    def _err_loop(self):
        try:
            for line in self.proc.stderr:
                self.stderr += line.decode(errors="replace")
                if len(self.stderr) > 8000:
                    self.stderr = self.stderr[-8000:]
        except Exception:
            pass

    def _send(self, obj):
        data = json.dumps(obj).encode()
        with self.lock:
            self.proc.stdin.write(b"Content-Length: %d\r\n\r\n" % len(data) + data)
            self.proc.stdin.flush()

    def _request(self, method, params, timeout=None):
        with self.cond:
            rid = self.next_id
            self.next_id += 1
        self._send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params})
        deadline = time.time() + (timeout or self.timeout)
        with self.cond:
            while rid not in self.replies:
                if self.proc.poll() is not None:
                    raise RuntimeError("lean --server exited: %s" % self.stderr[-300:])
                if not self.cond.wait(max(0.05, deadline - time.time())):
                    if time.time() > deadline:
                        raise TimeoutError("%s timed out" % method)
        return self.replies.pop(rid)

    def _notify(self, method, params):
        self._send({"jsonrpc": "2.0", "method": method, "params": params})

    def alive(self):
        return self.proc is not None and self.proc.poll() is None

    # ---- checking
    def check(self, code, timeout=None, settle=2.0):
        """Check `code` in one shared session file.

        The same gitignored file is kept open and changed in place, so Mathlib is
        imported once and each later check costs only elaboration. Two things
        make the result conclusive: the `didSave` (without it the server leaves
        the file at its pre-elaboration, empty batch — the old "false claim came
        back verified" bug) and the `$/lean/fileProgress` completion. An absent
        batch is therefore never read as "no errors"; the caller falls back to
        the one-shot run.
        """
        with self.check_lock:
            self.version += 1
            version = self.version
            path, uri = self.path, self.uri
            with open(path, "w") as f:
                f.write(code)
            if not self.opened:
                self._notify("textDocument/didOpen", {"textDocument": {
                    "uri": uri, "languageId": "lean4", "version": version,
                    "text": code}})
                self.opened = True
            else:
                self._notify("textDocument/didChange", {
                    "textDocument": {"uri": uri, "version": version},
                    "contentChanges": [{"text": code}]})
            self._notify("textDocument/didSave", {"textDocument": {"uri": uri}})

            limit = timeout or self.timeout
            deadline = time.time() + limit
            done0 = self.done_seq
            batch = None
            with self.cond:
                while True:
                    batch = next((m for m in reversed(self.msgs)
                                  if m["params"].get("uri") == uri
                                  and m["params"].get("version") == version), None)
                    has_diag = bool(batch and batch["params"].get("diagnostics"))
                    if has_diag or self.done_seq > done0:
                        break
                    if self.proc.poll() is not None:
                        raise RuntimeError("lean --server exited: %s" % self.stderr[-300:])
                    if not self.cond.wait(max(0.05, deadline - time.time())):
                        if time.time() > deadline:
                            break           # inconclusive: the caller falls back
                # let any follow-up batch for the same version land
                last = time.monotonic()
                while time.monotonic() - last < settle and time.monotonic() < deadline:
                    self.cond.wait(0.1)
                    newer = next((m for m in reversed(self.msgs)
                                  if m["params"].get("uri") == uri
                                  and m["params"].get("version") == version), None)
                    if newer is not batch:
                        batch = newer
                        last = time.monotonic()

            diags = []
            for d in (batch or {}).get("params", {}).get("diagnostics", []):
                sev = d.get("severity")
                diags.append({
                    "severity": {1: "error", 2: "warning"}.get(sev, "info"),
                    "line": d.get("range", {}).get("start", {}).get("line", 0) + 1,
                    "col": d.get("range", {}).get("start", {}).get("character", 0) + 1,
                    "message": d.get("message", "").strip(),
                })
            # `conclusive` requires an elaboration-complete signal (fileProgress)
            # or a batch that actually carries diagnostics.
            conclusive = (self.done_seq > done0) or bool(
                batch and batch["params"].get("diagnostics"))
            return diags, "\n".join(d["message"] for d in diags), conclusive

    def calibrate(self):
        """Prove the server can tell a false claim from a true one.

        A verdict is only taken from the server when this passes; otherwise the
        one-shot `lake env lean` run decides. This is what closes the LSP path:
        the empty-diagnostics race can no longer turn a false claim into a
        verified one, whatever the protocol timing.
        """
        if self.trusted is not None:
            return self.trusted
        header = "import Mathlib\n\n"
        try:
            bad, _, cb = self.check(header + "example : (1 : ℚ) = 2 := by norm_num",
                                    timeout=180)
            good, _, cg = self.check(header + "example : (1 : ℚ) = 1 := by norm_num",
                                     timeout=180)
        except Exception:
            self.trusted = False
            return False
        bad_err = any(d["severity"] == "error" for d in bad)
        good_err = any(d["severity"] == "error" for d in good)
        self.trusted = bool(cb and cg and bad_err and not good_err)
        return self.trusted

    def stop(self):
        try:
            self._request("shutdown", None, timeout=5)
            self._notify("exit", None)
        except Exception:
            pass
        try:
            if self.proc:
                self.proc.terminate()
        except Exception:
            pass
        self.proc = None


_SERVER = None
_SERVER_LOCK = threading.Lock()


def server():
    """The shared server for this project, started on first use."""
    global _SERVER
    with _SERVER_LOCK:
        if _SERVER is not None and _SERVER.alive():
            return _SERVER
        if _SERVER is not None:
            _SERVER.stop()
        _SERVER = LeanServer(project_dir()).start()
        atexit.register(lambda: _SERVER and _SERVER.stop())
        return _SERVER


def stop_server():
    global _SERVER
    with _SERVER_LOCK:
        if _SERVER is not None:
            _SERVER.stop()
            _SERVER = None


def server_trusted():
    """Whether the live server has passed calibration (`None` if not running or
    not yet calibrated)."""
    with _SERVER_LOCK:
        return None if _SERVER is None else _SERVER.trusted


def parse_diagnostics(output):
    """Lean prints `file:line:col: severity: message`; fold continuation lines
    into the message they belong to."""
    diags = []
    for line in output.splitlines():
        m = re.match(r"^(?P<file>[^:]+):(?P<line>\d+):(?P<col>\d+):\s*(?P<sev>error|warning|info|information)\s*:\s*(?P<msg>.*)$",
                     line)
        if m:
            diags.append({
                "severity": "error" if m.group("sev") == "error" else
                            "warning" if m.group("sev") == "warning" else "info",
                "line": int(m.group("line")),
                "col": int(m.group("col")),
                "message": m.group("msg").strip(),
            })
        elif diags and line.strip() and not line.startswith(" "):
            diags[-1]["message"] += "\n" + line.strip()
    return diags


def persistent_mode():
    """How the persistent LSP server may be used.

    `"auto"` (default) — use it for verdicts once it calibrates, else fall back
    to the one-shot run; `"require"` — as auto, but report if it is unusable;
    `"off"` — always one-shot. Whatever the mode, a non-conclusive server check
    falls back, so a verdict can never come from a missing diagnostics batch.
    """
    v = os.environ.get("RMATH_LEAN_SERVER", "auto").strip().lower()
    if v in ("0", "false", "no", "off", "none"):
        return "off"
    if v in ("1", "true", "yes", "on", "require"):
        return "require"
    return "auto"


def persistent_enabled():
    return persistent_mode() != "off"


def run_lean(code, timeout=600, filename=None, persistent=None):
    """Run Lean on `code` inside the project. Returns a result dict.

    `persistent=True` uses the LSP server (fast, opt-in, not trusted for
    verdicts); otherwise a one-shot `lake env lean` run decides."""
    if persistent is None:
        persistent = persistent_enabled()
    header = "" if code.lstrip().startswith("import") else "import Mathlib\n\n"
    body = header + code
    if persistent and available():
        try:
            t0 = time.time()
            srv = server()
            # A verdict is only taken from the server once it has proved it can
            # tell a false claim from a true one, and only from a conclusive check.
            if not srv.calibrate():
                raise RuntimeError("LSP server failed calibration")
            diags, _, conclusive = srv.check(body, timeout=timeout)
            if not conclusive:
                raise RuntimeError("LSP server returned no diagnostics batch")
            errors = [d for d in diags if d["severity"] == "error"]
            return {"status": "ok" if not errors else "error", "output": "",
                    "diagnostics": diags, "exit": 0 if not errors else 1,
                    "errors": errors, "warnings": [d for d in diags if d["severity"] == "warning"],
                    "via": "server", "seconds": round(time.time() - t0, 2)}
        except Exception as e:
            # fall through to the one-shot path, remembering why
            one = run_lean(code, timeout=timeout, filename=filename, persistent=False)
            one["server_error"] = str(e)
            return one
    return _run_lean_once(body, code, timeout, filename)


def _run_lean_once(body, code, timeout=600, filename=None):
    """One `lake env lean` process (slow: reloads Mathlib)."""
    lk = lake()
    if lk is None:
        return {"status": "error", "error": "Lean is not installed (no lake on PATH or in ~/.elan/bin)",
                "output": "", "diagnostics": []}
    project = project_dir()
    if not os.path.isdir(project):
        return {"status": "error", "error": "no Lean project at %s" % project,
                "output": "", "diagnostics": []}
    name = filename or "RMathCheck.lean"
    path = os.path.join(project, name)
    try:
        with open(path, "w") as f:
            f.write(body)
    except OSError as e:
        return {"status": "error", "error": str(e), "output": "", "diagnostics": []}
    try:
        proc = subprocess.run([lk, "env", "lean", name], cwd=project, capture_output=True,
                              text=True, timeout=timeout)
        out = (proc.stdout or "") + (proc.stderr or "")
        diags = parse_diagnostics(out)
        ok = proc.returncode == 0 and not any(d["severity"] == "error" for d in diags)
        # `sorry` exits 0 with a warning; a sorryAx proof proves nothing.
        if ok and "declaration uses 'sorry'" in out:
            ok = False
        return {"status": "ok" if ok else "error", "output": out.strip(),
                "diagnostics": diags, "exit": proc.returncode,
                "errors": [d for d in diags if d["severity"] == "error"],
                "warnings": [d for d in diags if d["severity"] == "warning"]}
    except subprocess.TimeoutExpired:
        return {"status": "error", "error": "Lean timed out after %ss" % timeout,
                "output": "", "diagnostics": []}
    except OSError as e:
        return {"status": "error", "error": str(e), "output": "", "diagnostics": []}


# ---------------------------------------------------------------- translation

_NUM = re.compile(r"(?<![\w.])(\d+)")
# Juxtaposition is multiplication in OutputForm: `1/2 Sqrt[2]`, `2 x`, `x y`.
# Juxtaposition is multiplication in OutputForm — including a numeral after
# another factor, as in `1/2 1/Sqrt[x]` (which parses as (1/2)*(1/Sqrt[x])).
_IMPLICIT = re.compile(
    r"(?<=[0-9A-Za-z_\)])\s+(?=(?:[A-Za-z_][A-Za-z_0-9]*|\d|\()(?!\s*\.))")


# Operations that preserve the value of the expression they wrap, so a result
# from them can be checked as an equality between the two translated forms.
TRANSPARENT = {"Factor", "Expand", "ExpandAll", "Together", "Cancel", "Collect",
               "Simplify", "FullSimplify", "Apart", "Distribute"}

# Machine approximations cannot be checked exactly.
REFUSE_FUNCS = {"N"}


def _rmath_to_python(text):
    t = text.strip()
    t = t.replace("∞", "Infinity")
    t = t.replace("π", "Pi")
    t = re.sub(r"(\d+)!", r"Factorial(\1)", t)          # postfix factorial
    # Factor[e] / Expand[e] keep the value of e: drop the head so the result is
    # checked as an equality between the two forms.
    for name in sorted(TRANSPARENT, key=len, reverse=True):
        t = re.sub(r"\b%s\s*\[" % name, "(", t)
    t = re.sub(r"\b([A-Za-z][A-Za-z0-9]*)\s*\[", r"\1(", t)  # calls: Sin[x] -> Sin(x)
    t = t.replace("[", "(").replace("]", ")")
    t = t.replace("{", "(").replace("}", ")")
    # Juxtaposition without whitespace, as rMath input allows: `2x²`, `2(x+1)`,
    # `(x+1)(x-1)`. The leading lookbehind keeps identifiers that merely end in a
    # digit (e.g. `Log2`) and decimals (`1.5`) from being split.
    t = re.sub(r"(?<![A-Za-z_0-9.])([0-9]+)(?=[A-Za-z(])", r"\1*", t)
    t = re.sub(r"(?<=\))(?=[A-Za-z0-9(])", "*", t)
    t = t.replace("^", "**")
    t = t.replace("&&", " and ").replace("||", " or ")
    # insert multiplication for juxtaposed factors, but never before a call
    def fix(m):
        return "*"
    out = []
    i = 0
    while True:
        m = _IMPLICIT.search(t, i)
        if not m:
            out.append(t[i:])
            break
        nxt = t[m.end():]
        name = re.match(r"[A-Za-z_][A-Za-z_0-9]*", nxt)
        # `f (x)` where f is a known function is a call, not a product
        prev = t[:m.start()]
        pname = re.search(r"([A-Za-z_][A-Za-z_0-9]*)\s*$", prev)
        is_call = bool(name) and pname and (pname.group(1) in FUNCS or pname.group(1) in CONSTS)
        out.append(t[i:m.end()] if is_call else t[i:m.start()] + "*")
        i = m.end() if is_call else m.end()
    return "".join(out)


class Translator:
    """rMath text -> (Lean expression, type, free variables)."""

    def __init__(self, ascribe=False, ty=None, complex=False):
        self.vars = {}
        self.real = False
        self.complex = complex
        self.error = None
        # Second pass: every numeral carries its type, so Lean cannot default
        # a bare `1/2` to ℕ (where 1/2 = 0 and false claims become provable).
        self.ascribe = ascribe
        self.ty = ty

    def _num(self, text, natural=False):
        if not self.ascribe or not self.ty:
            return text
        if natural:
            return "((%s : ℕ) : %s)" % (text, self.ty)
        return "(%s : %s)" % (text, self.ty)

    def expr(self, node):
        if isinstance(node, ast.Expression):
            return self.expr(node.body)
        if isinstance(node, ast.Constant):
            if isinstance(node.value, bool):
                return "True" if node.value else "False"
            if isinstance(node.value, int):
                return self._num(str(node.value))
            if isinstance(node.value, float):
                self.real = True
                return repr(node.value)
            raise ValueError("unsupported literal %r" % (node.value,))
        if isinstance(node, ast.Name):
            n = node.id
            if n in ("Pi", "E"):
                self.real = True
                return CONSTS[n]
            if n == "I":
                if not self.complex:
                    raise ValueError("complex numbers are not translated")
                return "Complex.I"
            if n in ("True", "False"):
                return n
            self.vars[n] = self.vars.get(n, None)
            return n
        if isinstance(node, ast.UnaryOp):
            if isinstance(node.op, ast.USub):
                return "(-%s)" % self.expr(node.operand)
            if isinstance(node.op, ast.UAdd):
                return self.expr(node.operand)
            if isinstance(node.op, ast.Not):
                return "(¬ %s)" % self.expr(node.operand)
            raise ValueError("unsupported unary operator")
        if isinstance(node, ast.BinOp):
            if isinstance(node.op, ast.Pow):
                # Lean's ^ takes a ℕ exponent, whatever the ambient type is.
                outer = self.ty
                if self.ascribe:
                    self.ty = "ℕ"
                b = self.expr(node.right)
                self.ty = outer
                a = self.expr(node.left)
            else:
                a, b = self.expr(node.left), self.expr(node.right)
            op = {ast.Add: "+", ast.Sub: "-", ast.Mult: "*", ast.Div: "/",
                  ast.Pow: "^", ast.Mod: "%"}.get(type(node.op))
            if op is None:
                raise ValueError("unsupported operator %s" % type(node.op).__name__)
            return "(%s %s %s)" % (a, op, b)
        if isinstance(node, ast.Compare):
            a = self.expr(node.left)
            parts = []
            left = node.left
            for op, right in zip(node.ops, node.comparators):
                sym = {ast.Eq: "=", ast.NotEq: "≠", ast.Lt: "<", ast.LtE: "≤",
                       ast.Gt: ">", ast.GtE: "≥"}.get(type(op))
                if sym is None:
                    raise ValueError("unsupported comparison")
                r = self.expr(right)
                parts.append("%s %s %s" % (a, sym, r))
                a = r
            return " ∧ ".join("(%s)" % p for p in parts) if len(parts) > 1 else parts[0]
        if isinstance(node, ast.BoolOp):
            op = " ∧ " if isinstance(node.op, ast.And) else " ∨ "
            return "(" + op.join("(%s)" % self.expr(v) for v in node.values) + ")"
        if isinstance(node, ast.Call):
            if not isinstance(node.func, ast.Name):
                raise ValueError("unsupported call")
            fname = node.func.id
            if fname == "Rational" and len(node.args) == 2:
                return "((%s) / (%s) : ℚ)" % (self.expr(node.args[0]), self.expr(node.args[1]))
            if fname == "Factorial" and len(node.args) == 1:
                outer = self.ty
                if self.ascribe:
                    self.ty = "ℕ"          # the argument of Nat.factorial is a ℕ
                inner = self.expr(node.args[0])
                self.ty = outer
                if self.ascribe and outer:
                    return "((Nat.factorial %s : ℕ) : %s)" % (inner, outer)
                return "Nat.factorial %s" % inner
            if fname == "Sqrt" and self.complex and len(node.args) == 1:
                # A negative radicand is a genuine complex square root; keep it
                # in ℂ rather than letting ℝ collapse it to 0.
                return "Complex.sqrt (%s)" % self.expr(node.args[0])
            target = FUNCS.get(fname)
            if target is None:
                raise ValueError("no Lean translation for %s" % fname)
            if target.startswith("Real."):
                self.real = True
            args = " ".join("(%s)" % self.expr(a) for a in node.args)
            return "%s %s" % (target, args) if args else target
        raise ValueError("unsupported construct %s" % type(node).__name__)

    def translate(self, text):
        """Returns (lean_expression, type_name, sorted variable names)."""
        self.vars, self.real, self.error = {}, False, None
        for bad in REFUSE_FUNCS:
            if re.search(r"\b%s\s*\[" % bad, text):
                raise ValueError("%s[...] is a machine approximation; "
                                 "Lean can only check exact statements" % bad)
        py = _rmath_to_python(text)
        try:
            tree = ast.parse(py, mode="eval")
        except SyntaxError as e:
            raise ValueError("cannot parse %r as rMath: %s" % (text, e.msg))
        lean = self.expr(tree)
        ty = "ℝ" if self.real else "ℚ"
        return lean, ty, sorted(v for v in self.vars)


class DerivGen:
    """Build a `HasDerivAt` proof term (and the matching function form) for an
    rMath expression in one variable.

    Mathlib has no `deriv` tactic here, so the proof is wired explicitly:
    hasDerivAt_pow / hasDerivAt_id / hasDerivAt_const for the leaves, then
    HasDerivAt.add / .sub / .mul / .const_mul / .neg and .sin / .cos / .exp.
    """

    def __init__(self, var, ty="ℝ"):
        self.var = var
        self.ty = ty
        # Side conditions the lemmas require (a non-zero denominator, log or
        # root argument), collected while walking and emitted as hypotheses.
        self.hyps = []

    def side_condition(self, value_text):
        """Record `value ≠ 0` and return the hypothesis name.

        The *reduced* value is used on purpose: `x ≠ 0` is what `field_simp`
        recognises, while `id x ≠ 0` (defeq) is not.
        """
        name = "h%d" % (len(self.hyps) + 1)
        self.hyps.append((name, "(%s ≠ 0)" % value_text))
        return name

    def has_var(self, node):
        for n in ast.walk(node):
            if isinstance(n, ast.Name) and n.id == self.var:
                return True
        return False

    def const_text(self, node):
        t = Translator(ascribe=True, ty=self.ty)
        return t.expr(node)

    def gen(self, node):
        """Returns (proof term, function form, value form).

        The value form is the expression the function takes at the point, used
        for side conditions (see `side_condition`).

        The function form is written exactly as the lemma states it, because
        `rw [h.deriv]` matches syntactically: `hasDerivAt_id` is about `id`,
        `const_mul` about `fun y => c * d y`, `sin` about `fun x => Real.sin (f x)`.
        """
        if isinstance(node, ast.Name) and node.id == self.var:
            return ("hasDerivAt_id %s" % self.var, "id", self.var)
        if isinstance(node, ast.Constant) and isinstance(node.value, (int, float)) \
                and not isinstance(node.value, bool):
            c = self.const_text(node)
            # hasDerivAt_const takes the point first, then the constant.
            return ("hasDerivAt_const %s (%s)" % (self.var, c), "(fun _ : ℝ => %s)" % c, c)
        if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Pow):
            if isinstance(node.right, ast.Constant) and isinstance(node.right.value, int) \
                    and not isinstance(node.right.value, bool) and node.right.value >= 0:
                n = node.right.value
                (t, f, v) = self.gen(node.left)
                if isinstance(node.left, ast.Name) and node.left.id == self.var:
                    return ("hasDerivAt_pow %d %s" % (n, self.var),
                            "(fun %s : ℝ => %s ^ (%d : ℕ))" % (self.var, self.var, n),
                            "(%s ^ (%d : ℕ))" % (v, n))
                # u^n through HasDerivAt.pow
                return ("((%s).pow %d)" % (t, n), "(%s ^ (%d : ℕ))" % (f, n), "(%s ^ (%d : ℕ))" % (v, n))
            if isinstance(node.right, ast.UnaryOp) and isinstance(node.right.op, ast.USub) \
                    and isinstance(node.right.operand, ast.Constant) and node.right.operand.value == 1:
                # u⁻¹ through HasDerivAt.inv
                (t, f, v) = self.gen(node.left)
                h = self.side_condition(v)
                return ("((%s).inv %s)" % (t, h), "%s⁻¹" % f, "%s⁻¹" % v)
            raise ValueError("only natural-number powers are supported")
        if isinstance(node, ast.BinOp):
            (tl, fl, vl) = self.gen(node.left)
            (tr, fr, vr) = self.gen(node.right)
            if isinstance(node.op, ast.Add):
                return ("((%s).add (%s))" % (tl, tr), "(%s + %s)" % (fl, fr), "(%s + %s)" % (vl, vr))
            if isinstance(node.op, ast.Sub):
                return ("((%s).sub (%s))" % (tl, tr), "(%s - %s)" % (fl, fr), "(%s - %s)" % (vl, vr))
            if isinstance(node.op, ast.Mult):
                if not self.has_var(node.left):
                    c = self.const_text(node.left)
                    return ("((%s).const_mul (%s))" % (tr, c),
                            "(fun %s : ℝ => %s * (%s %s))" % (self.var, c, fr, self.var),
                            "(%s * %s)" % (c, vr))
                if not self.has_var(node.right):
                    c = self.const_text(node.right)
                    return ("((%s).const_mul (%s))" % (tl, c),
                            "(fun %s : ℝ => %s * (%s %s))" % (self.var, c, fl, self.var),
                            "(%s * %s)" % (c, vl))
                return ("((%s).mul (%s))" % (tl, tr), "(%s * %s)" % (fl, fr), "(%s * %s)" % (vl, vr))
            if isinstance(node.op, ast.Div):
                h = self.side_condition(vr)
                return ("((%s).div (%s) %s)" % (tl, tr, h), "(%s / %s)" % (fl, fr), "(%s / %s)" % (vl, vr))
            raise ValueError("no derivative rule for this operation")
        if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.USub):
            (t, f, v) = self.gen(node.operand)
            return ("((%s).neg)" % t, "(-%s)" % f, "(-%s)" % v)
        if isinstance(node, ast.Call) and isinstance(node.func, ast.Name):
            name = node.func.id
            (t, f, v) = self.gen(node.args[0])
            forms = {"Sin": "Real.sin", "Cos": "Real.cos", "Exp": "Real.exp",
                     "Log": "Real.log", "Sqrt": "Real.sqrt"}
            methods = {"Sin": "sin", "Cos": "cos", "Exp": "exp"}
            if name in forms:
                if name in ("Log", "Sqrt"):
                    # both need the argument to be non-zero
                    h = self.side_condition(v)
                    return ("((%s).%s %s)" % (t, methods.get(name, name.lower()), h),
                            "(fun %s : ℝ => %s (%s %s))" % (self.var, forms[name], f, self.var),
                            "%s (%s)" % (forms[name], v))
                return ("((%s).%s)" % (t, methods[name]),
                        "(fun %s : ℝ => %s (%s %s))" % (self.var, forms[name], f, self.var),
                        "%s (%s)" % (forms[name], v))
            raise ValueError("no derivative rule for %s (Sin, Cos, Exp, Log, Sqrt)" % name)
        raise ValueError("no derivative rule for this construct")


def derivative_statement(lhs_text, rhs_text, var):
    """Build a Lean statement proving D[f, x] = g via HasDerivAt."""
    try:
        body = ast.parse(_rmath_to_python(lhs_text), mode="eval").body
    except SyntaxError as e:
        raise ValueError("cannot parse %r: %s" % (lhs_text, e.msg))
    if not (isinstance(body, ast.Call) and isinstance(body.func, ast.Name)
            and body.func.id == "D" and len(body.args) == 2):
        raise ValueError("not a derivative")
    f = body.args[0]
    gen = DerivGen(var)
    term, fun, _ = gen.gen(f)
    g = Translator(ascribe=True, ty="ℝ").expr(ast.parse(_rmath_to_python(rhs_text), mode="eval"))
    prop = "deriv %s %s = %s" % (fun, var, g)
    binders = "(x : ℝ)"
    for name, stmt in gen.hyps:
        binders += " (%s : %s)" % (name, stmt)
    # `simpa using h.deriv` closes the easy cases; otherwise rewrite with the
    # lemma's derivative and finish algebraically, with a `simp` step to reduce
    # `id x` and `* 1` that `ring_nf` leaves alone.
    code = (QUIET + "example %s : %s := by\n" % (binders, prop) +
            "  have h := (%s)\n"
            "  solve\n"
            "    | simpa using h.deriv\n"
            "    | (rw [h.deriv]; simp)\n"
            "    | (rw [h.deriv]; ring_nf; simp)\n"
            "    | (rw [h.deriv]; simp; ring_nf)\n"
            "    | (rw [h.deriv]; ring_nf)\n"
            "    | (rw [h.deriv]; ring)\n"
            "    | (rw [h.deriv]; norm_num)\n"
            "    | (rw [h.deriv]; field_simp; ring)\n"
            "    | (rw [h.deriv]; field_simp; ring_nf)\n"
            "    | (rw [h.deriv]; field_simp [*]; ring)\n"
            "    | (rw [h.deriv]; field_simp; simp; ring)\n"
            "    | (rw [h.deriv]; field_simp; simp; ring_nf)\n"
            "    | (rw [h.deriv]; simp; field_simp; ring)\n"
            "    | (rw [h.deriv]; field_simp; simp [id_eq]; ring)\n"
            "    | (rw [h.deriv]; simp only [id_eq]; field_simp)\n"
            "    | (rw [h.deriv]; simp only [id_eq]; field_simp; ring)\n" % (term,))
    return (code, prop, "ℝ", [])


def _calculus(lhs_text):
    """Recognise D[...] / Integrate[...] on the left-hand side."""
    try:
        body = ast.parse(_rmath_to_python(lhs_text), mode="eval").body
    except SyntaxError:
        return None
    if not (isinstance(body, ast.Call) and isinstance(body.func, ast.Name)):
        return None
    name = body.func.id
    if name == "Integrate" and len(body.args) == 2:
        target, spec = body.args
        if isinstance(spec, ast.Tuple) and len(spec.elts) == 3:
            var, a, b = spec.elts
            if isinstance(var, ast.Name):
                return ("definite", target, var.id, a, b)
        if isinstance(spec, ast.Name):
            return ("indefinite", target, spec.id, None, None)
    if name == "D" and len(body.args) == 2 and isinstance(body.args[1], ast.Name):
        return ("derivative", body.args[0], body.args[1].id, None, None)
    if name == "D" and len(body.args) == 2 and isinstance(body.args[1], ast.Tuple):
        # D[f, {x, n}]: the nth derivative.
        spec = body.args[1].elts
        if (len(spec) == 2 and isinstance(spec[0], ast.Name)
                and isinstance(spec[1], ast.Constant)
                and isinstance(spec[1].value, int) and spec[1].value >= 1):
            return ("derivative_n", body.args[0], spec[0].id, spec[1].value, None)
    return None


# Tactics that closed a definite integral of a power in testing. `solve` only
# accepts an alternative that closes the goal, so a failing lemma list is safe.
INTEGRAL_TACTICS = "norm_num [integral_pow] | norm_num | simp [integral_pow]"


def calculus_statement(lhs_text, rhs_text):
    """Build a Lean statement for a calculus result, or raise ValueError with
    the reason it cannot be checked yet."""
    found = _calculus(lhs_text)
    if found is None:
        return None
    kind, target, var, a, b = found
    if rhs_text is None:
        raise ValueError("nothing to verify: the input has no right-hand side")
    if kind == "definite":
        t = Translator(ascribe=True, ty="ℝ")
        t.vars = {var: None}
        f = t.expr(ast.parse(_rmath_to_python(_to_text(target)), mode="eval")
                   if isinstance(target, str) else target)
        r = Translator(ascribe=True, ty="ℝ")
        rhs = r.expr(ast.parse(_rmath_to_python(rhs_text), mode="eval"))
        prop = "(∫ %s in (%s)..%s, %s) = %s" % (
            var,
            Translator(ascribe=True, ty="ℝ").expr(a),
            Translator(ascribe=True, ty="ℝ").expr(b),
            f, rhs)
        return (QUIET + "example : %s := by\n  solve | %s\n" % (prop, INTEGRAL_TACTICS),
                prop, "ℝ", [])
    if kind == "indefinite":
        # ∫ f dx = F is checked by proving (d/dx) F = f — the same HasDerivAt
        # wiring used for D[...], with F on the left instead of the integrand.
        target_text = _to_text(target) if not isinstance(target, str) else target
        try:
            return derivative_statement("D[%s, %s]" % (rhs_text, var), target_text, var)
        except ValueError as e:
            raise ValueError("cannot check this antiderivative: %s" % e)
    if kind == "derivative_n":
        # D[f, {x, n}] = g becomes `iteratedDeriv n f x = g`. A pure power (or a
        # constant multiple of one, or a constant) closes with
        # `iteratedDeriv_pow`; the remaining arithmetic is finished by `ring`.
        # Shapes that need `iteratedDeriv_add`/`_mul` side conditions are left
        # for Lean to reject, so a verdict is never invented.
        n = a
        t = Translator(ascribe=True, ty="ℝ")
        t.vars = {var: None}
        f = t.expr(target if not isinstance(target, str)
                   else ast.parse(_rmath_to_python(target), mode="eval"))
        r = Translator(ascribe=True, ty="ℝ")
        r.vars = {var: None}
        g = r.expr(ast.parse(_rmath_to_python(rhs_text), mode="eval"))
        prop = "iteratedDeriv %d (fun %s : ℝ => %s) %s = %s" % (n, var, f, var, g)
        tactics = ("norm_num [iteratedDeriv_pow, iteratedDeriv_const_mul, "
                   "iteratedDeriv_const] | "
                   "(norm_num [iteratedDeriv_pow, iteratedDeriv_const_mul, "
                   "iteratedDeriv_const]; ring) | ring_nf")
        # `autoImplicit false` means the evaluation point must be bound explicitly.
        return (QUIET + "example (%s : ℝ) : %s := by\n  solve | %s\n"
                % (var, prop, tactics), prop, "ℝ", [])
    try:
        return derivative_statement(lhs_text, rhs_text, var)
    except ValueError as e:
        raise ValueError("cannot check this derivative: %s" % e)


def _to_text(node):
    return ast.unparse(node)


def _balanced_brackets(text):
    """True when brackets/braces/parens are balanced (string literals respected)."""
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


# Heads that only wrap an expression for display; Lean checks what is inside.
DISPLAY_WRAPPERS = ("Steps", "Trace")


def unwrap_display(text):
    """The inner expression of a display wrapper `Steps[e]` / `Trace[e]`, or
    None when `text` is not one. A cell showing a derivation should be verified
    by the thing it derives, not by the Step list."""
    s = text.strip()
    for head in DISPLAY_WRAPPERS:
        prefix = head + "["
        if s.startswith(prefix) and s.endswith("]"):
            inner = s[len(prefix):-1].strip()
            if inner and _balanced_brackets(inner):
                return inner
    return None


def is_solve(text):
    """True when `text` is a `Solve[...]` call."""
    try:
        body = ast.parse(_rmath_to_python(text), mode="eval").body
    except SyntaxError:
        return False
    return isinstance(body, ast.Call) and getattr(body.func, "id", "") == "Solve"


def _split_top_level(text):
    """Split at top-level commas, respecting brackets/braces and strings."""
    parts, depth, cur, i = [], 0, [], 0
    while i < len(text):
        c = text[i]
        if c == '"':
            cur.append(c)
            i += 1
            while i < len(text):
                cur.append(text[i])
                if text[i] == "\\" and i + 1 < len(text):
                    cur.append(text[i + 1])
                    i += 2
                    continue
                if text[i] == '"':
                    i += 1
                    break
                i += 1
            continue
        if c in "[{(":
            depth += 1
        elif c in "]})":
            depth -= 1
        if c == "," and depth == 0:
            parts.append("".join(cur))
            cur = []
            i += 1
            continue
        cur.append(c)
        i += 1
    parts.append("".join(cur))
    return [p.strip() for p in parts if p.strip()]


def _parse_root_ast(result_text, numeric=None):
    """Root values from a Solve result's OutputForm,
    `{{x -> r1}, {x -> r2}}` -> `[r1, r2]` (ASTs). OutputForm keeps `Sqrt[…]`
    and `^`, which the translator already understands.

    `numeric` is the OutputForm of `N[result]`; roots with an imaginary part are
    dropped, since Lean checks equations over ℝ (and `Real.sqrt` of a negative
    is 0, so such a claim would be false)."""
    sym_texts = _root_texts(result_text)
    num_texts = _root_texts(numeric) if numeric else []
    roots = []
    for i, txt in enumerate(sym_texts):
        if i < len(num_texts) and "I" in num_texts[i]:
            continue
        try:
            roots.append(ast.parse(_rmath_to_python(txt), mode="eval").body)
        except SyntaxError:
            continue
    return roots


def _root_texts(result_text):
    """The rMath text of each root in a `{{x -> r}, ...}` result."""
    s = (result_text or "").strip()
    if s.startswith("{") and s.endswith("}"):
        s = s[1:-1]
    out = []
    for item in _split_top_level(s):
        if item.startswith("{") and item.endswith("}"):
            item = item[1:-1]
        if "->" not in item:
            continue
        rhs = item.partition("->")[2].strip()
        if rhs:
            out.append(rhs)
    return out


class _SubstituteVar(ast.NodeTransformer):
    def __init__(self, name, repl):
        self.name = name
        self.repl = repl

    def visit_Name(self, node):
        if node.id == self.name:
            return ast.copy_location(self.repl, node)
        return node


def _complex_solve_statement(left, right, var, all_texts, relation):
    """All roots are non-real: substitute them back over ℂ (where they live).
    Item 3 of the Lean backlog."""
    parts, vs = [], set()
    for txt in all_texts:
        try:
            root = ast.parse(_rmath_to_python(txt), mode="eval").body
        except SyntaxError:
            continue
        t = _SubstituteVar(var, root)
        lhs = t.visit(copy.deepcopy(left))
        rhs = t.visit(copy.deepcopy(right))
        tl = Translator(ascribe=True, ty="ℂ", complex=True)
        rl = Translator(ascribe=True, ty="ℂ", complex=True)
        parts.append("(%s %s %s)" % (tl.expr(lhs), relation, rl.expr(rhs)))
        vs |= set(tl.vars) | set(rl.vars)
    if not parts:
        raise ValueError("every root is complex and could not be translated")
    vs.discard(var)
    prop = " ∧ ".join(parts) if len(parts) > 1 else parts[0]
    names = sorted(vs)
    binders = ("(" + " ".join(names) + " : ℂ) ") if names else ""
    code = ("-- no real roots: the roots are substituted back over ℂ\n" + QUIET +
            "example %s: %s := by\n  solve | %s\n" % (binders, prop, SOLVE_TACTICS))
    return (code, prop, "ℂ", names)


def solve_statement(solve_text, result_form, numeric=None, relation="="):
    """A `Solve[eq, x]` claim: each (real) root the kernel returned satisfies
    `eq`. The kernel found the roots; Lean is asked the independent question —
    is each of these actually a root? — by substituting it back into the
    equation. Non-real roots are dropped (this is an ℝ check). Returns None when
    `solve_text` is not a Solve call.
    """
    try:
        body = ast.parse(_rmath_to_python(solve_text), mode="eval").body
    except SyntaxError as e:
        raise ValueError("cannot parse %r as rMath: %s" % (solve_text, e.msg))
    if not (isinstance(body, ast.Call) and getattr(body.func, "id", "") == "Solve"):
        return None
    if len(body.args) != 2 or not isinstance(body.args[1], ast.Name):
        raise ValueError("cannot check this Solve: only one equation in one "
                         "variable is supported")
    eq, var = body.args[0], body.args[1].id
    if not (isinstance(eq, ast.Compare) and len(eq.ops) == 1
            and isinstance(eq.ops[0], ast.Eq)):
        raise ValueError("cannot check this Solve: the input must be an "
                         "equation `a == b`")
    left, right = eq.left, eq.comparators[0]
    all_texts = _root_texts(result_form)
    roots = _parse_root_ast(result_form, numeric)
    if not roots:
        if all_texts:
            # No real roots: check them over ℂ instead of refusing.
            return _complex_solve_statement(left, right, var, all_texts, relation)
        raise ValueError("Solve returned no roots to check")
    skipped = len(all_texts) - len(roots)
    # ℝ when the equation or any root is real-valued (Sqrt, Pi, Real.*).
    real = False
    for r in roots:
        p = Translator()
        p.expr(r)
        real = real or p.real
    p = Translator()
    p.expr(eq)
    real = real or p.real
    ty = "ℝ" if real else "ℚ"
    parts = []
    vs = set()
    for root in roots:
        t = _SubstituteVar(var, root)
        lhs = t.visit(copy.deepcopy(left))
        rhs = t.visit(copy.deepcopy(right))
        tl = Translator(ascribe=True, ty=ty)
        rl = Translator(ascribe=True, ty=ty)
        parts.append("(%s %s %s)" % (tl.expr(lhs), relation, rl.expr(rhs)))
        vs |= set(tl.vars) | set(rl.vars)
    vs.discard(var)
    prop = " ∧ ".join(parts) if len(parts) > 1 else parts[0]
    names = sorted(vs)
    binders = ("(" + " ".join(names) + " : %s) " % ty) if names else ""
    note = ""
    if skipped:
        note = "-- checked %d of %d roots; the other %d are not real\n" % (
            len(roots), len(all_texts), skipped)
    code = note + QUIET + "example %s: %s := by\n  solve | %s\n" % (
        binders, prop, SOLVE_TACTICS)
    return (code, prop, ty, names)


_PREAMBLE = None


def _with_import(code):
    """The file body with `import Mathlib` guaranteed (the inlined preamble has
    no import of its own)."""
    return code if code.lstrip().startswith("import") else "import Mathlib\n\n" + code


def _preamble():
    """`lean/RMathLemmas.lean` without its `import` line, inlined into each
    generated file so no separate `lake build` step is needed."""
    global _PREAMBLE
    if _PREAMBLE is None:
        try:
            with open(os.path.join(project_dir(), "RMathLemmas.lean")) as f:
                lines = f.readlines()
            _PREAMBLE = "".join(
                l for l in lines if not l.strip().startswith("import ")).strip() + "\n\n"
        except OSError:
            _PREAMBLE = ""
    return _PREAMBLE


def _named(code):
    """Name the generated proof so `#print axioms` can audit it."""
    return code.replace("example", "theorem rmath_check", 1)


def _finalized(code):
    return _preamble() + _named(code) + "\n#print axioms rmath_check\n"


def _display(code):
    """What the panel shows: the claim without the linter preamble."""
    return _named(code).replace(QUIET, "") + "\n#print axioms rmath_check"


_AXIOM_RE = re.compile(r"depends on axioms:\s*\[([^\]]*)\]")


def _parse_axioms(output):
    m = _AXIOM_RE.search(output or "")
    if not m:
        return []
    return [a.strip() for a in m.group(1).split(",") if a.strip()]


def _hammer_enabled():
    return os.environ.get("RMATH_LEAN_HAMMER", "") in ("1", "true", "yes")


def _hammer_run(code, timeout):
    """A second attempt with external hammers, for projects that add Duper or
    lean-smt to `lakefile.toml`. Harmless (just unproved) when they are absent."""
    alt = re.sub(r"solve \| .*", "solve | duper | lean_smt | aesop | grind", code, count=1)
    if alt == code:
        return None
    res = run_lean(_finalized(alt), timeout=timeout, filename="RMathVerify.lean",
                   persistent=False)
    res["lean"] = _finalized(alt)
    res["lean_display"] = _display(alt)
    res["axioms"] = _parse_axioms(res.get("output", ""))
    res["via"] = "hammer"
    if res.get("status") == "ok":
        res["outcome"] = "verified"
    elif res.get("status") == "error" and res.get("errors"):
        res["outcome"] = "unproved"
    else:
        res["outcome"] = res.get("status", "error")
    return res


def _transparent_unwrap(text):
    """Reduce `Collect[e, x]`, `Apart[e, x]`, `Factor[e]`, … to the expression
    whose value they preserve. Without this their extra argument reaches the
    Python translation and becomes a tuple (`unsupported construct Tuple`)."""
    s = text.strip()
    for name in sorted(TRANSPARENT, key=len, reverse=True):
        if s.startswith(name) and s[len(name):].lstrip().startswith("["):
            inner = s[s.index("[") + 1:].rstrip()
            if inner.endswith("]"):
                inner = inner[:-1]
            parts = _split_top_level(inner)
            if parts:
                return _transparent_unwrap(parts[0])
    return s


_FINITE = re.compile(r"^(Sum|Product)\s*\[(.*)\]\s*$", re.S)


def _expand_finite(text):
    """`Sum[f, {i, a, b}]` / `Product[f, {i, a, b}]` with integer bounds become
    the explicit sum/product, which `norm_num` can evaluate directly."""
    m = _FINITE.match(text.strip())
    if not m:
        return text
    head, inner = m.group(1), m.group(2)
    parts = _split_top_level(inner)
    if len(parts) != 2:
        return text
    spec = parts[1].strip().strip("{}")
    spec = _split_top_level(spec)
    if len(spec) != 3 or not isinstance(spec[0], str):
        return text
    var = spec[0].strip()
    try:
        lo, hi = int(spec[1]), int(spec[2])
    except ValueError:
        return text
    if hi < lo or hi - lo > 40:
        return text
    body = parts[0]
    terms = [re.sub(r"\b%s\b" % re.escape(var), str(k), body) for k in range(lo, hi + 1)]
    op = " + " if head == "Sum" else " * "
    return "(" + op.join("(%s)" % t for t in terms) + ")"


def statement(lhs_text, rhs_text=None, relation="="):
    """Build a Lean `example` proving lhs = rhs (or a claim about lhs)."""
    # `Collect[e, x]` etc. preserve the value of their first argument, and a
    # finite `Sum`/`Product` is an explicit sum/product.
    lhs_text = _expand_finite(_transparent_unwrap(lhs_text))
    if rhs_text is not None:
        rhs_text = _transparent_unwrap(rhs_text)

    def tree(text):
        try:
            return ast.parse(_rmath_to_python(text), mode="eval")
        except SyntaxError as e:
            raise ValueError("cannot parse %r as rMath: %s" % (text, e.msg))

    def typed(text, ty, extra_vars):
        t = Translator(ascribe=True, ty=ty)
        t.vars = {v: None for v in extra_vars}
        return t.expr(tree(text))

    if rhs_text is None:
        # Without a right-hand side the input must itself be a claim.
        probe = Translator()
        probe.expr(tree(lhs_text))
        if not isinstance(ast.parse(_rmath_to_python(lhs_text), mode="eval").body,
                          (ast.Compare, ast.BoolOp)):
            raise ValueError("nothing to verify: %r is not a claim "
                             "(expected an equation or inequality)" % lhs_text)
        ty = "ℝ" if probe.real else "ℚ"
        vs = sorted(probe.vars)
        prop = typed(lhs_text, ty, vs)
    else:
        probe_l, probe_r = Translator(), Translator()
        probe_l.expr(tree(lhs_text))
        probe_r.expr(tree(rhs_text))
        ty = "ℝ" if (probe_l.real or probe_r.real) else "ℚ"
        vs = sorted(set(probe_l.vars) | set(probe_r.vars))
        prop = "%s %s %s" % (typed(lhs_text, ty, vs), relation, typed(rhs_text, ty, vs))
    binders = ("(" + " ".join(vs) + " : %s) " % ty) if vs else ""
    return (QUIET + "example %s: %s := by\n  solve | %s\n" % (binders, prop, TACTICS),
            prop, ty, vs)


_VERIFY_CACHE = {}


def premise_search(query, backend="loogle", timeout=8):
    """Search Mathlib premises for a goal or a natural-language query.

    Returns `{"status": ..., "results": [...]}`; an unreachable backend is an
    `error` with no results (the bridge must work offline). `loogle` is
    structure search (e.g. `Real.sqrt ?a * Real.sqrt ?a`), `leansearch` is
    natural language.
    """
    import urllib.parse
    import urllib.request
    q = urllib.parse.quote(query)
    urls = {
        "loogle": "https://loogle.lean-lang.org/json?q=" + q,
        "leansearch": "https://leansearch.net/search?query=" + q + "&num_results=8",
    }
    url = urls.get(backend)
    if not url:
        return {"status": "error", "error": "unknown backend %r" % backend, "results": []}
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "rMath/0.1"})
        with urllib.request.urlopen(req, timeout=timeout) as r:
            data = json.load(r)
    except Exception as e:
        return {"status": "error", "error": str(e), "backend": backend, "results": []}
    return {"status": "ok", "backend": backend, "results": _search_results(backend, data)}


def _search_results(backend, data):
    """Normalise the two search backends to `[{name, type, module}]`."""
    out = []

    def add(name, ty="", module=""):
        if name and not any(x["name"] == name for x in out):
            out.append({"name": name, "type": ty, "module": module})

    if backend == "loogle":
        for hit in (data.get("hits") or []):
            add(hit.get("name", ""), hit.get("type", ""), hit.get("module", ""))
    else:
        items = data if isinstance(data, list) else (data.get("results") or [])
        for hit in items:
            if not isinstance(hit, dict):
                continue
            inner = hit.get("result") if isinstance(hit.get("result"), dict) else hit
            add(inner.get("name", ""), inner.get("type", ""), inner.get("module", ""))
    return out


def build_statement(lhs_text, rhs_text=None, result_form=None, numeric=None,
                    relation="="):
    """The `(code, proposition, type, variables)` for a claim, or `ValueError`.
    Shared by the one-shot verifier and the fast LSP explorer."""
    built = calculus_statement(lhs_text, rhs_text) if rhs_text is not None else None
    if built is None and result_form:
        built = solve_statement(lhs_text, result_form, numeric=numeric, relation=relation)
    return built if built else statement(lhs_text, rhs_text, relation)


def verify(lhs_text, rhs_text=None, relation="=", timeout=600, use_cache=True,
           result_form=None, numeric=None):
    """Verify an rMath claim in Lean. Returns a result dict with `outcome`:
    verified | unproved | cannot verify | error.

    `result_form` is the InputForm of the cell's result; for a `Solve[...]` cell
    it carries the roots, and the claim checked is that each of them really is a
    root (by substitution). The authoritative one-shot `lake env lean` run
    decides: its exit status and diagnostics come from the elaborator itself,
    with no notification-timing assumptions. Results are cached per claim, so
    re-checking a cell is free.
    """
    ck = (lhs_text, rhs_text, relation, result_form, numeric)
    if use_cache and ck in _VERIFY_CACHE:
        cached = dict(_VERIFY_CACHE[ck])
        cached["cached"] = True
        return cached
    try:
        code, prop, ty, vs = build_statement(lhs_text, rhs_text, result_form=result_form,
                                             numeric=numeric, relation=relation)
    except ValueError as e:
        return {"status": "error", "error": str(e), "lean": None, "outcome": "cannot verify"}
    full = _finalized(code)
    # Fast path: the persistent LSP server, but only if it has calibrated and
    # returns a conclusive batch. Anything else falls through to the one-shot
    # run, so a server timing issue can never decide a verdict.
    res = None
    if persistent_mode() != "off" and available():
        try:
            srv = server()
            if srv.calibrate():
                t0 = time.time()
                diags, _, conclusive = srv.check(_with_import(full), timeout=timeout)
                if conclusive:
                    errors = [d for d in diags if d["severity"] == "error"]
                    res = {"status": "ok" if not errors else "error",
                           "output": "\n".join(srv.logs[-40:]),
                           "diagnostics": diags, "errors": errors,
                           "warnings": [d for d in diags if d["severity"] == "warning"],
                           "via": "server", "seconds": round(time.time() - t0, 2)}
        except Exception:
            res = None
    if res is None:
        res = run_lean(full, timeout=timeout, filename="RMathVerify.lean",
                       persistent=False)
    res.setdefault("via", "one-shot")
    res["lean"] = full
    res["lean_display"] = _display(code)
    res["proposition"] = prop
    res["type"] = ty
    res["variables"] = vs
    res["axioms"] = _parse_axioms(
        (res.get("output") or "") + "\n" +
        "\n".join(d.get("message", "") for d in (res.get("diagnostics") or [])))
    if res.get("status") == "ok":
        res["outcome"] = "verified"
    elif res.get("status") == "error" and res.get("errors"):
        res["outcome"] = "unproved"
    else:
        res["outcome"] = res.get("status", "error")
    # A proof that used `sorry` proves nothing; never report it as verified.
    if "sorryAx" in res.get("axioms", []) or any(
            "sorry" in (d.get("message") or "").lower()
            for d in (res.get("diagnostics") or [])):
        res["outcome"] = "unproved"
    # Item 8: optional external-hammer fallback (env-gated).
    if res.get("outcome") == "unproved" and _hammer_enabled():
        alt = _hammer_run(code, timeout)
        if alt is not None and alt.get("outcome") == "verified":
            res = alt
    if use_cache and res.get("outcome") in ("verified", "unproved"):
        _VERIFY_CACHE[ck] = dict(res)
    return res
