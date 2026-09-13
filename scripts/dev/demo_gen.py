#!/usr/bin/env python3
"""Validate (and so maintain) demo/demo.mt.

The demo is its own source of truth. This replays it against the kernel
**statement by statement in one session**, exactly as the notebook's Open does,
so examples that depend on earlier definitions or on files written by earlier
statements are checked in context. A line that stops evaluating, starts
producing messages, or is silently unevaluated is reported rather than left in
the file as a lie.

    make demo-gen          # reports; exits non-zero on any problem

Layout understood:
    Print["== heading =="]                       a section banner
    Print["  <expr>  =  ", <expr>]                an example: must evaluate
    <anything else>                              a statement (definitions,
                                                 Do/Print loops, Put/Plot, notes)
"""

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DEMO = os.path.join(ROOT, "demo/demo.mt")
KERNEL = os.path.join(ROOT, "rmath/target/release/rmath")

EXAMPLE = re.compile(r'^Print\["  (?P<label>.*?)  =  ", (?P<expr>.*)\]$')

# Results that are *correctly* unchanged: symbolic constants and held forms.
UNCHANGED_OK = {
    "Pi", "E", "GoldenRatio", "EulerGamma", "Infinity", "Degree",
    "Hold[1 + 1]", "Sequence[1, 2]", "3 I", "1 + I",
    "SinIntegral[Infinity]", "Gamma[0]",
}
# Deliberate demonstrations of error handling: these *should* produce messages.
MESSAGES_OK = {"1/0", "0/0"}


class Kernel:
    def __init__(self, workdir):
        self.proc = subprocess.Popen([KERNEL, "--server"], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                     text=True, cwd=workdir)
        self.n = 0

    def eval(self, text):
        self.n += 1
        self.proc.stdin.write(json.dumps({"id": self.n, "op": "eval", "input": text}) + "\n")
        self.proc.stdin.flush()
        reply = self.proc.stdout.readline()
        if not reply:
            return {"status": "error", "error": "kernel stopped responding"}
        return json.loads(reply)

    def close(self):
        try:
            self.proc.stdin.write(json.dumps({"op": "shutdown"}) + "\n")
            self.proc.stdin.flush()
        except Exception:
            pass


def main():
    text = open(DEMO).read()
    lines = text.splitlines()
    # Run in a private copy of demo/: the file-I/O section writes files, and
    # doing that in the repository races a running app doing the same thing.
    workdir = tempfile.mkdtemp(prefix="rmath-demo-")
    try:
        shutil.copytree(os.path.join(ROOT, "demo"), os.path.join(workdir, "demo"))
    except OSError:
        os.makedirs(os.path.join(workdir, "demo"), exist_ok=True)
    k = Kernel(workdir)
    problems = []
    examples = 0
    statements = 0
    in_comment = False
    for line_no, raw in enumerate(lines, 1):
        line = raw.strip()
        if in_comment:
            if "*)" in line:
                in_comment = False
            continue
        if line.startswith("(*"):
            if "*)" not in line:
                in_comment = True
            continue
        if not line:
            continue
        m = EXAMPLE.match(line)
        if m:
            examples += 1
            expr = m.group("expr")
            # Run the line as the file would (side effects, printing) ...
            k.eval(line)
            # ... then inspect the expression's own result in context.
            r = k.eval(expr)
            result = (r.get("result") or "").strip()
            msgs = [x.get("text") for x in (r.get("messages") or [])]
            if r.get("status") != "ok":
                problems.append((line_no, expr, "error: %s" % r.get("error", "")))
            elif result == expr.strip() and expr not in UNCHANGED_OK:
                problems.append((line_no, expr, "unevaluated"))
            elif msgs and expr not in MESSAGES_OK:
                problems.append((line_no, expr, "messages: " + "; ".join(msgs)[:60]))
        else:
            statements += 1
            r = k.eval(line)
            if r.get("status") != "ok":
                problems.append((line_no, line[:60], "statement error: %s" % r.get("error", "")))
    k.close()
    shutil.rmtree(workdir, ignore_errors=True)

    print("checked %d examples and %d statements in demo/demo.mt" % (examples, statements))
    for line_no, expr, why in problems:
        print("  line %d: %s\n      %s" % (line_no, expr[:64], why))
    if problems:
        print("DEMO-GEN: FAIL (%d problem(s))" % len(problems))
        return 1
    print("DEMO-GEN: PASS (every example evaluates cleanly, in context)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
