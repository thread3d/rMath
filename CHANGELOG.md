# Changelog

All notable changes to rMath. Versions before 0.1.1 are the original history;
`git log` has the full record of those 60-odd commits.

## 0.2.0 — the Lean milestone

Lean 4 + Mathlib can now actually decide a claim on a machine where `$HOME` is
read-only, and it is set up by two commands instead of a conversation. Alongside
it, ten correctness and honesty fixes found by auditing the kernel against
Mathematica's semantics.

### Kernel correctness

- **`PrimeQ` no longer answers `False` for an integer it cannot test.** The
  Miller-Rabin test is machine-word only; a larger argument now stays
  unevaluated with `PrimeQ::bigint` instead of denying `2^89 - 1` and `2^127 - 1`
  (both prime). A symbolic argument stays unevaluated too.
- **`Chop` returns its argument.** It used to wrap every non-tiny real in its own
  head (`Chop[0.1]` printed as `Chop[0.1]`) and refused lists; it now chops reals
  below the tolerance, complex components, and inside lists and expressions.
- **Chained comparisons are one n-ary relation.** `3 > 2 > 1` parsed as
  `Greater[Greater[3, 2], 1]` and evaluated to `Greater[True, 1]`; it is now
  `Greater[3, 2, 1]` → `True`, with mixed chains as the conjunction of their
  pairs. `Equal`/`Unequal` are n-ary to match.
- **Exact values at multiples of `Pi/6`, `Pi/4`, `Pi/3` and `Pi/2`** for `Sin`,
  `Cos`, `Tan`, `Cot`, `Sec`, `Csc`, with `ComplexInfinity` where the denominator
  vanishes (`Tan[Pi/2]`, `Cot[0]`), plus the hyperbolics at 0, the inverse
  functions at 0 and ±1 and ±1/2, `Exp[1] = E` and `Exp[Log[x]] = x`. Definite
  integrals over `[0, Pi/2]` now close: `Integrate[Cos[x], {x, 0, Pi/2}]` → `1`.
- **`GCD` and `LCM` work on arbitrary-size integers** (`GCD[2^64, 2^70]` → `2^64`);
  they used to read every argument through a machine-word conversion.

### Honesty of the tools

- `make demo-gen` now reports an example whose result only *re-spells* its input
  (`Sin[Pi/6]` → `Sin[1/6 Pi]`), which is how five unevaluated examples sat in
  `demo/demo.mt` while the validator reported PASS.
- `N::meprec` no longer claims arbitrary-precision numbers are unimplemented.
- The README names the modules that exist (there is no `repl.rs`), states the
  real test counts, and lists the `PrimeQ` and trigonometric-table limits.

### Notebook API

- `/api/save`, `/api/save-svg` and `/api/open` refuse a path outside the working
  directory, and KaTeX serving uses `commonpath` rather than a prefix test. The
  API answers every local client by design, so the path handling was the only
  thing between a request body and the filesystem.

### Lean integration

- `available()` requires `lake` **and** a built `Mathlib.olean`; a Lean without
  Mathlib parses `import Mathlib` as an unknown module, so the old check reported
  a toolchain that could not verify anything.
- Toolchain discovery prefers elan's shims (`$ELAN_BIN`, `$ELAN_HOME/bin`,
  `<repo>/.elan/bin`, `~/.elan/bin`) over a system `lean`/`lake` of another
  version, and every run gets a matching `PATH`/`ELAN_HOME`/`MATHLIB_CACHE_DIR`.
  `lake` and `lean` in a bare shell still resolve to the system binaries, which is
  why the bridge sets the environment itself.
- `make lean-status` reports elan, the pinned toolchain, Mathlib and the cache and
  names the command that fixes what is missing; `make lean-setup` installs
  whatever is missing and is idempotent. Both work with the toolchain, Mathlib and
  its cache inside the repository, which is the only writable place when `$HOME`
  is read-only.
- `make lean-verify` fails with the reason and the fix instead of skipping
  silently; `--skip` keeps the old behaviour for machines without Lean.

### Tests

- Rust: 63 unit tests + 6 integration tests, 471 golden cases (was 408).
- Python: 167 tests (was 132), including `tests/test_demo_gen.py` and the Lean
  doctor/toolchain discovery.
- `make lean-verify`: 38/38, proving the arithmetic, algebra, derivative,
  integral and `Solve` claims and rejecting the three false ones.

## 0.1.0

First recorded version: symbolic kernel in Rust (expression engine, pattern
matching, exact arithmetic, CAS builtins), Python front end (notebook, SVG plots,
JSON API, MCP server, Lean bridge, harness panel), macOS app, and the Lean/Mathlib
project.
