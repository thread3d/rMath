#!/bin/bash
# Time the evaluator's acceptance workloads, N runs each, and print the
# per-run milliseconds.  The performance note (docs/PERFORMANCE.md) requires
# three runs on each side of a change, because single-run noise on this
# machine has been mistaken for a win before.
#
#   scripts/dev/bench.sh [runs]
#
# Uses the release binary the Makefile builds, so its timings are the ones a
# user sees.  Output is machine-greppable: one "expr-ms <label> <ms>" line per
# run, then a median per label.
set -u
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BIN="${BENCH_BIN:-$ROOT/rmath/target/release/rmath}"
RUNS="${1:-3}"

if [ ! -x "$BIN" ]; then
  echo "missing $BIN — run 'make release' first" >&2
  exit 1
fi

# label<TAB>expression.  Keep these the two workloads the note names.
CASES=(
  "expand40	Expand[(x + y + z)^40]"
  "nest1600	Nest[f, x, 1600]"
)

for case in "${CASES[@]}"; do
  label="${case%%	*}"
  expr="${case#*	}"
  cpus=()
  walls=()
  for _ in $(seq "$RUNS"); do
    # CPU time, not wall time: this machine runs a loaded desktop, and wall
    # time moved by 20% between two runs of the *same* binary. `%U + %S` is the
    # time the kernel charges to the process, so a neighbour cannot inflate it.
    read -r wall user sys < <(/usr/bin/time -f "%e %U %S" "$BIN" -e "$expr" 2>&1 >/dev/null | tail -1) \
      || { echo "FAILED: $label" >&2; exit 1; }
    cpu=$(awk -v u="$user" -v s="$sys" 'BEGIN { printf "%d", (u + s) * 1000 }')
    cpus+=("$cpu")
    walls+=("$(awk -v w="$wall" 'BEGIN { printf "%d", w * 1000 }')")
    echo "run-cpu-ms $label $cpu"
  done
  # Median (the note's bar is "three runs each side"; the median is what one
  # compares, and printing every run keeps a lucky sample visible).
  median() { printf '%s\n' "$@" | sort -n | sed -n "$(( ($# + 1) / 2 ))p"; }
  echo "median-cpu-ms $label $(median "${cpus[@]}")"
  echo "median-wall-ms $label $(median "${walls[@]}")"
done
