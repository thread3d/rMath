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
         "set_option linter.unnecessarySimpa false\n\n")

# Functions that are real-valued, forcing the ℝ interpretation.
REAL_FUNCS = {v for k, v in FUNCS.items() if v.startswith("Real.")}

# Used as `solve | ...`: only an alternative that *closes* the goal counts.
# `first` would stop at a tactic that merely simplified, leaving the goal open.
TACTICS = ("norm_num | ring | ring_nf | positivity | linarith | nlinarith | "
           "omega | simp | decide | rfl"
           if False else
           "norm_num | ring | ring_nf | positivity | linarith | nlinarith | "
           "omega | simp | decide | rfl")


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
        self.uri = "file://" + os.path.join(project, "RMathSession.lean")
        self.path = os.path.join(project, "RMathSession.lean")
        self.timeout = timeout
        self.proc = None
        self.version = 0
        self.opened = False
        self.seq = 0
        self.lock = threading.RLock()
        self.msgs = []
        self.cond = threading.Condition(self.lock)
        self.next_id = 1
        self.replies = {}
        self.diag_seq = 0
        self.done_seq = 0     # $/lean/fileProgress with no work left
        self.trace = []       # (method, summary) for debugging
        self.stderr = ""

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
    def check(self, code, timeout=None, settle=3.0):
        """Check `code`, waiting for this file's *elaborated* diagnostics.

        Each check gets a fresh URI and an explicit `didChange`: a plain
        `didOpen` makes the server publish an empty batch and the real
        diagnostics only afterwards, which is how a false claim once came back
        "verified". Matching on the fresh URI *and* version removes both the
        staleness and the too-early read.
        """
        with self.lock:
            self.seq += 1
            name = "RMathCheck_%d.lean" % self.seq
        path = os.path.join(self.project, name)
        uri = "file://" + path
        with open(path, "w") as f:
            f.write(code)
        self._notify("textDocument/didOpen", {"textDocument": {
            "uri": uri, "languageId": "lean4", "version": 1, "text": code}})

        limit = timeout or self.timeout
        deadline = time.time() + limit
        batch = None
        try:
            # The server answers `didOpen` with a pre-elaboration (empty) batch;
            # wait for that round to finish before asking for the change, or the
            # two collapse and the elaborated diagnostics never arrive.
            with self.cond:
                while not any(m["params"].get("uri") == uri for m in self.msgs):
                    if not self.cond.wait(max(0.05, min(5.0, deadline - time.time()))):
                        break
            time.sleep(0.3)
            self._notify("textDocument/didChange", {
                "textDocument": {"uri": uri, "version": 2},
                "contentChanges": [{"text": code}]})
            with self.cond:
                while True:
                    batch = next((m for m in reversed(self.msgs)
                                  if m["params"].get("uri") == uri
                                  and m["params"].get("version") == 2), None)
                    if batch is not None:
                        break
                    if self.proc.poll() is not None:
                        raise RuntimeError("lean --server exited: %s" % self.stderr[-300:])
                    if not self.cond.wait(max(0.05, deadline - time.time())):
                        if time.time() > deadline:
                            raise TimeoutError("no diagnostics for %s within %ss" % (name, limit))
                # let any follow-up batch for the same file/version land
                last = time.monotonic()
                while time.monotonic() - last < settle and time.monotonic() < deadline:
                    self.cond.wait(0.1)
                    newer = next((m for m in reversed(self.msgs)
                                  if m["params"].get("uri") == uri
                                  and m["params"].get("version") == 2), None)
                    if newer is not batch:
                        batch = newer
                        last = time.monotonic()
        finally:
            try:
                self._notify("textDocument/didClose", {"textDocument": {"uri": uri}})
            except Exception:
                pass
            try:
                os.unlink(path)
            except OSError:
                pass

        diags = []
        for d in (batch or {}).get("params", {}).get("diagnostics", []):
            sev = d.get("severity")
            diags.append({
                "severity": {1: "error", 2: "warning"}.get(sev, "info"),
                "line": d.get("range", {}).get("start", {}).get("line", 0) + 1,
                "col": d.get("range", {}).get("start", {}).get("character", 0) + 1,
                "message": d.get("message", "").strip(),
            })
        return diags, "\n".join(d["message"] for d in diags)

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


def persistent_enabled():
    """The LSP server is opt-in.

    It is much faster (Mathlib loads once: ~0.8s per check instead of ~10s), but
    in testing it published *empty* diagnostic batches for files with genuine
    errors — for example `example : (1 : ℚ) = 2 := by norm_num` came back with
    no diagnostics, which would report a false claim as verified. Verdicts
    therefore use the one-shot run, whose exit status is authoritative. Set
    RMATH_LEAN_SERVER=1 to use the server for interactive checking only.
    """
    return os.environ.get("RMATH_LEAN_SERVER", "") not in ("", "0", "false")


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
            diags, _ = server().check(body, timeout=timeout)
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

    def __init__(self, ascribe=False, ty=None):
        self.vars = {}
        self.real = False
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
                raise ValueError("complex numbers are not translated")
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
        raise ValueError("antiderivatives need a derivative proof, which is not "
                         "wired up yet (this Mathlib has no `deriv` tactic)")
    try:
        return derivative_statement(lhs_text, rhs_text, var)
    except ValueError as e:
        raise ValueError("cannot check this derivative: %s" % e)


def _to_text(node):
    return ast.unparse(node)


def statement(lhs_text, rhs_text=None, relation="="):
    """Build a Lean `example` proving lhs = rhs (or a claim about lhs)."""
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


def verify(lhs_text, rhs_text=None, relation="=", timeout=600, use_cache=True):
    """Verify an rMath claim in Lean. Returns a result dict with `outcome`:
    verified | unproved | cannot verify | error.

    The authoritative one-shot `lake env lean` run decides: its exit status and
    diagnostics come from the elaborator itself, with no notification-timing
    assumptions. Results are cached per claim, so re-checking a cell is free.
    """
    ck = (lhs_text, rhs_text, relation)
    if use_cache and ck in _VERIFY_CACHE:
        cached = dict(_VERIFY_CACHE[ck])
        cached["cached"] = True
        return cached
    try:
        built = calculus_statement(lhs_text, rhs_text) if rhs_text is not None else None
        if built is None and _calculus(lhs_text) is not None and rhs_text is not None:
            built = None
        code, prop, ty, vs = built if built else statement(lhs_text, rhs_text, relation)
    except ValueError as e:
        return {"status": "error", "error": str(e), "lean": None, "outcome": "cannot verify"}
    res = run_lean(code, timeout=timeout, filename="RMathVerify.lean", persistent=False)
    res["lean"] = code
    res["proposition"] = prop
    res["type"] = ty
    res["variables"] = vs
    if res.get("status") == "ok":
        res["outcome"] = "verified"
    elif res.get("status") == "error" and res.get("errors"):
        res["outcome"] = "unproved"
    else:
        res["outcome"] = res.get("status", "error")
    if use_cache and res.get("outcome") in ("verified", "unproved"):
        _VERIFY_CACHE[ck] = dict(res)
    return res
