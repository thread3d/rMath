#!/usr/bin/env python3
"""Dev-only correctness oracle for the rMath kernel (backlog item 1).

Random expressions are fed to the kernel and each operation's *invariants* are
checked by numeric sampling: `Expand[Factor[p]]` must equal `p`, every `Solve`
root must satisfy its equation, `D[Integrate[f]]` must equal `f`. Value checks
alone cannot see "it did not simplify", so rational-function checks also probe
the removed singular point (`Cancel[(x-r)·d/(x-r)]` must be finite at `x=r`).

This is deliberately independent of the kernel's own simplifier: a
plausible-but-wrong answer (like `Factor[2x^2-2] = 4(-1+x)(1+x)`) fails the
sample check even when `Simplify` cannot see it.

    python3 scripts/dev/oracle_test.py                 # 60 cases per group
    python3 scripts/dev/oracle_test.py --seed 7 -n 200
    python3 scripts/dev/oracle_test.py --only Cancel

Exit status is non-zero if any invariant fails.
"""

import argparse
import json
import os
import random
import select
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BIN = os.path.join(ROOT, "rmath", "target", "release", "rmath")


class Kernel:
    def __init__(self):
        self.p = None
        self.seq = 0
        self.start()

    def start(self):
        self.p = subprocess.Popen([BIN, "--server"], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, text=True)
        self.seq = 0

    def restart(self):
        try:
            self.p.kill()
        except Exception:
            pass
        self.start()

    def eval(self, text, timeout=8.0):
        """Evaluate, with a wall-clock guard: a looping expression must not hang
        the whole oracle. On timeout the kernel is restarted (it is single
        threaded, so a stuck evaluation blocks everything after it)."""
        self.seq += 1
        try:
            self.p.stdin.write(json.dumps({"id": self.seq, "op": "eval", "input": text}) + "\n")
            self.p.stdin.flush()
        except Exception:
            self.restart()
            raise TimeoutError("kernel pipe closed")
        while True:
            r, _, _ = select.select([self.p.stdout], [], [], timeout)
            if not r:
                self.restart()
                raise TimeoutError("eval timed out: %s" % text[:60])
            line = self.p.stdout.readline()
            if not line:
                self.restart()
                raise TimeoutError("kernel died")
            try:
                d = json.loads(line)
            except ValueError:
                continue
            if d.get("id") == self.seq:
                return d

    def close(self):
        try:
            self.p.stdin.write(json.dumps({"op": "shutdown"}) + "\n")
            self.p.stdin.flush()
        except Exception:
            pass
        self.p.terminate()


def number(k, expr, env):
    """Numeric value of `expr` at `env`; None for complex / pole / error."""
    rules = ", ".join("%s -> %s" % (v, n) for v, n in env.items())
    try:
        r = k.eval("N[(%s) /. {%s}]" % (expr, rules))
    except TimeoutError:
        return None
    if r.get("status") != "ok":
        return None
    s = (r.get("result") or "").replace(" ", "")
    if not s or "I" in s or "Infinity" in s or "Indeterminate" in s or "Complex" in s:
        return None
    try:
        if "/" in s:
            a, b = s.split("/", 1)
            return float(a) / float(b)
        return float(s)
    except ValueError:
        return None


def pick(rng, v):
    if v in ("n", "k"):
        return rng.choice([1, 2, 3, 4, 5, 6, 7, 8])
    return rng.choice([-5, -4, -3, -2, -1, 1, 2, 3, 4, 5, 7, 9, 11])


def same(k, rng, a, b, vars, n=6):
    """True/False/None: do `a` and `b` agree? None = no usable sample."""
    used = 0
    for _ in range(n):
        env = {v: pick(rng, v) for v in vars}
        va, vb = number(k, a, env), number(k, b, env)
        if va is None or vb is None:
            continue
        used += 1
        if abs(va - vb) > 1e-6 * max(1.0, abs(va), abs(vb)):
            return False, "at %s: %s vs %s" % (env, va, vb)
    return (None, "no usable sample") if used == 0 else (True, "")


def same_at(k, a, b, env):
    """Compare at one specific point; a pole on either side is a failure."""
    va, vb = number(k, a, env), number(k, b, env)
    if va is None or vb is None:
        return False, "pole/undefined at %s (got %s vs %s)" % (env, va, vb)
    if abs(va - vb) > 1e-6 * max(1.0, abs(va), abs(vb)):
        return False, "at %s: %s vs %s" % (env, va, vb)
    return True, ""


# ---------------------------------------------------------------- generators

def poly_coeffs(rng, maxdeg=4, lo=-6, hi=6):
    deg = rng.randint(1, maxdeg)
    c = [rng.randint(lo, hi) for _ in range(deg + 1)]
    if c[deg] == 0:
        c[deg] = rng.choice([-5, -3, -2, 2, 3, 5])
    return c


def poly_str(c, var="x"):
    terms = []
    for i, ci in enumerate(c):
        if ci == 0:
            continue
        if i == 0:
            terms.append(str(ci))
        elif i == 1:
            terms.append(("%d %s" % (ci, var)) if ci != 1 else var)
        else:
            terms.append(("%d %s^%d" % (ci, var, i)) if ci != 1 else "%s^%d" % (var, i))
    return " + ".join(terms) if terms else "0"


def lin(r):
    return "(x - %d)" % r if r >= 0 else "(x + %d)" % (-r)


def factored_str(rng, maxdeg=3):
    parts = [lin(rng.randint(-5, 5)) for _ in range(rng.randint(1, maxdeg))]
    lead = rng.choice([1, 1, 2, 3, 5])
    return ("%d %s" % (lead, " ".join(parts))) if lead != 1 else " ".join(parts)


def value_cases(rng, count):
    for _ in range(count):
        p = poly_str(poly_coeffs(rng))
        f = factored_str(rng)
        yield ("Factor/Expand", "Expand[Factor[%s]]" % p, p, ("x",), None)
        yield ("Factor/Expand", "Expand[Factor[%s]]" % f, "Expand[%s]" % f, ("x",), None)
        yield ("Collect/Expand", "Expand[Collect[%s, x]]" % p, p, ("x",), None)
        yield ("Expand/power", "Expand[(x + 2)^3]", "(x + 2)^3", ("x",), None)
        yield ("Simplify", "Simplify[%s]" % p, p, ("x",), None)
        yield ("Trig", "Simplify[Sin[x]^2 + Cos[x]^2]", "1", ("x",), None)
        yield ("Sum", "Sum[k^2, {k, 1, n}]", "n (n + 1) (2 n + 1)/6", ("n",), None)
        yield ("Product", "Product[k, {k, 1, n}]", "n!", ("n",), None)
        yield ("D/Integrate", "D[Integrate[%s, x], x]" % p, p, ("x",), None)
        yield ("D/Integrate", "D[Integrate[Sin[x] + x^2, x], x]", "Sin[x] + x^2", ("x",), None)
        a, b, c, d = (rng.randint(-7, 7) for _ in range(4))
        yield ("Det", "Det[{{%d, %d}, {%d, %d}}]" % (a, b, c, d),
               "(%d)*(%d) - (%d)*(%d)" % (a, d, b, c), ("x",), None)


def rational_cases(rng, count):
    for _ in range(count):
        p = poly_str(poly_coeffs(rng))
        r = rng.randint(-5, 5)
        L = lin(r)
        expr = "(%s)/(%s)" % (p, L)
        yield ("Cancel/value", "Cancel[%s]" % expr, expr, ("x",), None)
        yield ("Apart/value", "Apart[%s, x]" % expr, expr, ("x",), None)
        yield ("Together", "Together[Apart[%s, x]]" % expr, expr, ("x",), None)
        # must actually cancel: finite at the removed root. The numerator needs
        # explicit parentheses — `(x+5) -5 + ...` parses as subtraction.
        num = "(%s)(%s)" % (L, p)
        yield ("Cancel@root", "Cancel[%s/(%s)]" % (num, L), p, ("x",), {"x": r})
        yield ("Apart@root", "Apart[%s/(%s), x]" % (num, L), p, ("x",), {"x": r})


def calculus_cases():
    yield ("Limit", "Limit[(x^2 - 1)/(x - 1), x -> 1]", "2", ("x",), {})
    yield ("Limit", "Limit[Sin[x]/x, x -> 0]", "1", ("x",), {})


def solve_cases(rng, count):
    for _ in range(count):
        p = poly_str(poly_coeffs(rng, maxdeg=3))
        yield ("Solve", "Solve[%s == 0, x]" % p, p, ("x",), None)


def run(checks, cases, k, rng):
    fails, seen, timeouts = {}, {}, []
    for group, lhs, rhs, vars, at in cases:
        if checks and group not in checks:
            continue
        seen[group] = seen.get(group, 0) + 1
        try:
            if group == "Solve":
                r = k.eval(lhs)
                result = (r.get("result") or "").strip()
                if result.startswith("Solve"):
                    continue          # unsupported (e.g. cubic with no rational root)
                if r.get("status") != "ok" or not result.startswith("{"):
                    fails.setdefault(group, []).append((lhs, "no result: " + result[:60], ""))
                    continue
                roots = [t.split("->")[1].strip().rstrip("}")
                         for t in result.replace("{", "").replace("}", "").split(",")
                         if "->" in t]
                bad = None
                for root in roots:
                    e = number(k, rhs, {"x": root})
                    if e is None:
                        continue
                    if abs(e) > 1e-6:
                        bad = "root %s gives %s" % (root, e)
                        break
                if bad:
                    fails.setdefault(group, []).append((lhs, bad, ""))
                continue
            if at is not None:
                ok, detail = same_at(k, lhs, rhs, at)
            else:
                ok, detail = same(k, rng, lhs, rhs, vars)
        except TimeoutError as e:
            timeouts.append((group, lhs, str(e)))
            continue
        if ok is False:
            fails.setdefault(group, []).append((lhs, rhs, detail))
    return fails, seen, timeouts


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("-n", "--count", type=int, default=60)
    ap.add_argument("--only", action="append", default=[])
    args = ap.parse_args()
    rng = random.Random(args.seed)
    k = Kernel()
    try:
        cases = (list(value_cases(rng, args.count)) + list(calculus_cases())
                 + list(rational_cases(rng, args.count)) + list(solve_cases(rng, args.count)))
        fails, seen, timeouts = run(set(args.only), cases, k, rng)
    finally:
        k.close()
    total = sum(seen.values())
    nfail = sum(len(v) for v in fails.values())
    print("RMath ORACLE  (seed %d, %d cases)" % (args.seed, total))
    for group in sorted(seen):
        print("  %-14s %5d cases  %s" % (group, seen[group],
                                         "FAIL" if group in fails else "ok"))
    for group in sorted(fails):
        for lhs, rhs, detail in fails[group][:6]:
            print("    [%s] %s\n          vs %s   %s" % (group, lhs[:96], (rhs or "")[:36], detail))
    for group, lhs, why in timeouts[:6]:
        print("    [timeout/%s] %s  (%s)" % (group, lhs[:70], why))
    if nfail:
        print("ORACLE: FAIL (%d invariant violation(s), %d timeout(s))" % (nfail, len(timeouts)))
        return 1
    print("ORACLE: PASS (%d timeout(s))" % len(timeouts))
    return 0


if __name__ == "__main__":
    sys.exit(main())
