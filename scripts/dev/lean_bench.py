#!/usr/bin/env python3
"""Benchmark the Lean bridge over rMath's own corpus.

    python3 scripts/dev/lean_bench.py               # translate only (fast)
    python3 scripts/dev/lean_bench.py --verify      # run Lean on each claim
    python3 scripts/dev/lean_bench.py --verify --limit 20

It reads the golden `.mt` files (which carry `input => expected` pairs) and the
demo file (`Print["  <expr>  =  ", <expr>]`), builds a Lean statement for each,
and reports how many translate and how many Lean actually proves. This is the
yardstick for the tactic/premise work: a change that raises `verified` is good.
"""

import argparse
import os
import re
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, ROOT)
from rmathpy import lean  # noqa: E402

GOLDEN = os.path.join(ROOT, "rmath", "tests", "golden")
DEMO = os.path.join(ROOT, "demo", "demo.mt")
DEMO_EXAMPLE = re.compile(r'^Print\["\s\s(?P<label>.*?)\s\s=\s\s",\s*(?P<expr>.*)\]$')


def golden_pairs(path):
    """`input` / `=> expected` pairs from a golden file."""
    out, pending = [], None
    with open(path) as f:
        for raw in f:
            line = raw.rstrip("\n")
            if not line.strip() or line.lstrip().startswith("#"):
                continue
            if line.startswith("=>"):
                if pending is not None:
                    out.append((pending, line[2:].strip()))
                    pending = None
            else:
                pending = line.strip()
    return out


def demo_pairs(path):
    """Examples from the demo, checked against their own evaluation result."""
    out = []
    try:
        with open(path) as f:
            for line in f:
                m = DEMO_EXAMPLE.match(line.strip())
                if m:
                    out.append((m.group("expr"), None))
    except OSError:
        pass
    return out


def collect():
    """(source, lhs, rhs) triples. `rhs` is None for demo examples, whose
    right-hand side is the kernel's own result (translated as a claim only)."""
    items = []
    if os.path.isdir(GOLDEN):
        for name in sorted(os.listdir(GOLDEN)):
            if name.endswith(".mt"):
                for lhs, rhs in golden_pairs(os.path.join(GOLDEN, name)):
                    items.append((name, lhs, rhs))
    for lhs, rhs in demo_pairs(DEMO):
        items.append(("demo.mt", lhs, rhs))
    return items


def translate_any(lhs, rhs):
    """Try every statement builder the bridge has, mirroring `/api/lean/verify`.

    Returns True when a statement was built, False when the bench cannot model
    the case (a `Steps`/`Trace` cell is verified against its *evaluated* inner
    value, which needs a live kernel, not a golden right-hand side)."""
    if lean.unwrap_display(lhs) is not None:
        return False
    if rhs is not None and lean.is_solve(lhs):
        lean.solve_statement(lhs, rhs)
        return True
    if rhs is not None:
        try:
            if lean.calculus_statement(lhs, rhs) is not None:
                return True
        except ValueError:
            pass
    lean.statement(lhs, rhs)
    return True


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--verify", action="store_true", help="run Lean on each claim (slow)")
    ap.add_argument("--limit", type=int, default=0, help="stop after N claims")
    ap.add_argument("--timeout", type=int, default=120)
    args = ap.parse_args()

    items = collect()
    if args.limit:
        items = items[: args.limit]
    translated = refused = skipped = 0
    verified = unproved = errors = 0
    failures = []
    t0 = time.time()
    for src, lhs, rhs in items:
        try:
            if not translate_any(lhs, rhs):
                skipped += 1
                continue
            translated += 1
        except ValueError as e:
            refused += 1
            failures.append(("translate", src, lhs, str(e)))
            continue
        if not args.verify:
            continue
        r = lean.verify(lhs, rhs, timeout=args.timeout, use_cache=False)
        outcome = r.get("outcome")
        if outcome == "verified":
            verified += 1
        elif outcome == "unproved":
            unproved += 1
            failures.append(("unproved", src, lhs, ""))
        else:
            errors += 1
            failures.append(("error", src, lhs, (r.get("error") or outcome or "")[:80]))

    print("LEAN BENCH  (%d claims, %.1fs)" % (len(items), time.time() - t0))
    print("  translated : %d" % translated)
    print("  refused    : %d" % refused)
    print("  skipped    : %d  (Steps/Trace need a live kernel)" % skipped)
    if args.verify:
        print("  verified   : %d" % verified)
        print("  unproved   : %d" % unproved)
        print("  errors     : %d" % errors)
        if translated:
            print("  proof rate : %.1f%%" % (100.0 * verified / translated))
    if failures and not args.verify:
        for kind, src, lhs, why in failures[:15]:
            print("    %-9s %-16s %s (%s)" % (kind, src, lhs[:40], why[:50]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
