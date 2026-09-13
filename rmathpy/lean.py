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
import os
import re
import subprocess
import tempfile

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
    env = os.environ.get("RMATH_LEAN_PROJECT")
    if env and os.path.isdir(env):
        return env
    return os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "lean")


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


def run_lean(code, timeout=600, filename=None):
    """Run Lean on `code` inside the project. Returns a result dict."""
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
    header = "" if code.lstrip().startswith("import") else "import Mathlib\n\n"
    body = header + code
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
_IMPLICIT = re.compile(
    r"(?<=[0-9A-Za-z_\)])\s+(?=(?:[A-Za-z_][A-Za-z_0-9]*|\()(?!\s*\.))")


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
        return ("example : %s := by\n  solve | %s\n" % (prop, INTEGRAL_TACTICS), prop, "ℝ", [])
    if kind == "indefinite":
        raise ValueError("antiderivatives need a derivative proof, which is not "
                         "wired up yet (this Mathlib has no `deriv` tactic)")
    raise ValueError("derivative verification is not wired up yet (this Mathlib "
                     "has no `deriv` tactic; it needs explicit deriv lemmas)")


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
    return "example %s: %s := by\n  solve | %s\n" % (binders, prop, TACTICS), prop, ty, vs


def verify(lhs_text, rhs_text=None, relation="=", timeout=600):
    """Verify an rMath claim in Lean. Returns a result dict with `status`:
    verified | refuted | unproved | error."""
    try:
        built = calculus_statement(lhs_text, rhs_text) if rhs_text is not None else None
        if built is None and _calculus(lhs_text) is not None and rhs_text is not None:
            built = None
        code, prop, ty, vs = built if built else statement(lhs_text, rhs_text, relation)
    except ValueError as e:
        return {"status": "error", "error": str(e), "lean": None, "outcome": "cannot verify"}
    res = run_lean(code, timeout=timeout, filename="RMathVerify.lean")
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
    return res
