#!/usr/bin/env bash
# Bring Lean + Mathlib into this checkout, so `make lean-verify` can decide
# claims. Idempotent: run it after a fresh clone, or any time the pieces are
# missing — every step it already satisfies is skipped.
#
#   make lean-setup
#
# What it puts where (all gitignored):
#   <repo>/.elan             elan + the toolchain lean/lean-toolchain pins
#   <repo>/.mathlib-cache    lake exe cache get downloads (~450 MB)
#   <repo>/lean/.lake        Mathlib sources and the built oleans (~7.5 GB)
#
# ELAN_HOME and MATHLIB_CACHE_DIR default to those in-repo paths because a
# read-only $HOME (a sandbox, a container, some CI images) breaks ~/.elan and
# ~/.cache/mathlib. Export your own to install elsewhere.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
PIN=$(tr -d '[:space:]' < "$ROOT/lean/lean-toolchain")
export ELAN_HOME=${ELAN_HOME:-$ROOT/.elan}
export MATHLIB_CACHE_DIR=${MATHLIB_CACHE_DIR:-$ROOT/.mathlib-cache}
export PATH="$ELAN_HOME/bin:$PATH"

say() { printf '\n== %s\n' "$*"; }

say "elan in $ELAN_HOME (toolchain $PIN)"
if [ -x "$ELAN_HOME/bin/elan" ]; then
    echo "   already installed: $("$ELAN_HOME/bin/elan" --version)"
else
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    # The bootstrap script from the Lean project (a rustup-init.sh derivative).
    curl -fsSL https://elan.lean-lang.org/elan-init.sh -o "$tmp/elan-init.sh"
    ELAN_HOME="$ELAN_HOME" sh "$tmp/elan-init.sh" -y --no-modify-path \
        --default-toolchain "$PIN"
fi

say "toolchain $PIN"
elan toolchain install "$PIN"   # a no-op when it is already there

say "Mathlib cache ($MATHLIB_CACHE_DIR)"
mkdir -p "$MATHLIB_CACHE_DIR"
cd "$ROOT/lean"
lake exe cache get               # skips the files already decompressed

say "build"
lake build

say "check"
python3 "$ROOT/scripts/dev/lean_status.py"
