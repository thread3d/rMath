#!/usr/bin/env python3
"""Report whether this checkout can actually verify a claim in Lean.

Prints the pieces a Lean check needs — elan, the pinned toolchain, Mathlib, the
download cache — and the single command that fixes whatever is missing. Needs no
Lean and no network to run, so it is safe as a preflight.

    make lean-status          # this
    make lean-setup           # the fix it names

Exit status: 0 when Lean can verify, 1 when it cannot (the reason is on stdout).
"""

import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, ROOT)
from rmathpy import lean  # noqa: E402


def line(label, value, ok=None):
    mark = "" if ok is None else ("ok  " if ok else "MISSING ")
    if isinstance(value, bool):
        value = "yes" if value else "no"
    print("  %-10s %s%s" % (label + ":", mark, value if value else "-"))


def main():
    st = lean.status()
    line("project", st["project"])
    line("toolchain", st["toolchain"] or "(no lean/lean-toolchain)")
    line("elan", st["elan_version"] or st["elan"], bool(st["elan_version"]))
    line("installed", st["toolchain_installed"], st["toolchain_installed"])
    line("lake", st["lake"], st["lake"] is not None)
    line("mathlib", st["mathlib"], st["mathlib"] is not None)
    line("cache", st["mathlib_cache"])
    line("server", st["server_mode"])
    if st["available"]:
        print("LEAN-STATUS: READY (claims can be verified)")
        return 0
    print("LEAN-STATUS: NOT READY — %s" % (st["next_step"] or "Lean is unavailable"))
    return 1


if __name__ == "__main__":
    sys.exit(main())
