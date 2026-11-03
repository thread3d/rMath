# Evaluator performance: the change-tracking design note

The one performance item left. Written before any code, because the last two
attempts were coded first and measured neutral — the record of what was tried
belongs here rather than in a commit message nobody will find again.

> **Outcome (0.2.32): implemented, measured, reverted.** The design below does
> not work: its first rule is unsound (a node's own rewrite can undo a child's
> change, and trusting the child then spins the fixpoint loop), and the sound
> remainder measured *slower*. The numbers, the counterexample, and what the
> profile says the remaining cost actually is, are in the last section.

## What the profiles say

| workload | time (0.2.27) | what the profile shows |
|---|---|---|
| `Expand[(x + y + z)^40]` | 5.5–6.1 s | `same_q` was 22% before 0.2.17; now spread across `eval_node`, `eval_args` and allocator traffic |
| `Nest[f, x, 1600]` | 0.5 s | was 10.9 s before 0.2.10, when the "did you mean" hint lowercased every symbol name per node |

Two fixes landed and are measured: the hint memo (0.2.10) and the `HashMap` merge
of like terms/powers (0.2.17). Three were tried and produced **no measurable
change**, which is the reason this note exists:

- `FactorKey` no longer allocates a `String` per sort comparison — kept (free), no gain.
- `same_q` returns early on pointer/`Rc` identity — kept (free), no gain.
- `same_q_shallow` in `eval_node`'s change checks — kept (free), no gain.

So the cost is *not* a hot mistake any more. It is structural: the evaluator
re-walks subtrees it has already evaluated.

## Where the work actually goes

`eval()` iterates to a fixpoint:

```
loop {
    let (next, changed) = self.eval_once(&cur)?;
    if !changed { return Ok(next) }
    cur = next;
}
```

`eval_once` → `eval_node` evaluates head and arguments, normalizes, then dispatches
to up-values / down-values / built-ins, and after each dispatch answers *"is the
result the same as the input?"* with `same_q(&v, orig)`. That comparison is what
keeps the loop terminating: a rule that returns an expression identical to its
input must be reported as *unchanged*, or the loop would spin until
`$IterationLimit`.

For a node of size *k* the comparison is O(k), and it happens at every level of
every evaluation. Building a result bottom-up therefore costs O(size) per node,
i.e. O(n²) over an n-term result — which is exactly the shape of the remaining
`Expand` cost. The comparison cannot simply be dropped: it is the termination
proof.

## The design

Carry "changed" instead of inferring it, and keep a cheap comparison only where
the answer is genuinely unknown.

1. **Trust the children.** `eval_args` already knows which arguments changed
   (`achanged`). If any child changed, the rebuilt node differs from `orig` — no
   comparison needed.
2. **Ask the rule that ran.** `try_downvalues` and `call_builtin` are the only
   places a *new* expression appears. Give each a way to say "I rewrote" versus
   "I returned the same shape". A builtin that rebuilds its input from unchanged
   arguments (the common case: `Plus` on a sum that cannot be combined) returns a
   *fresh* `Rc` whose children are pointer-identical — so the answer is still
   cheap, but by inspecting the result's arguments rather than comparing trees.
3. **Compare only on doubt.** Where neither signal settles it, fall back to
   `same_q` as today. `same_q_shallow` (0.2.19) is the cheap first half of that
   fallback and already exists.
4. **Keep termination provable.** For every path that reports "unchanged", either
   the argument pointers prove it or `same_q` does. No path may report "changed"
   without evidence, because that is what would spin the loop.

## Acceptance criteria

- `Expand[(x + y + z)^40]` and `Nest[f, x, 1600]` measurably faster, with three
  runs each before and after (this project has been burned by single-run noise).
- **No semantic change anywhere**: `cargo test` (unit, golden, fuzz, ordering),
  `make oracle` on two seeds, `python3 -m unittest discover -s tests`,
  `make demo-gen`, and `make lean-verify` (the battery is the independent
  cross-check that evaluation still means the same thing).
- If the change does not measure, revert it and record that here, as with the
  three above.

## Why not memoise or hash-cons

- *Hash-consing every node* would make structural equality O(1), but it changes the
  representation everywhere (`Rc::new` → an interning table) and risks a lifetime
  and memory regression for a workload nobody has complained about.
- *A memo table keyed by node pointer* needs invalidation whenever a definition or
  attribute changes, which is a correctness surface this project cannot afford to
  get subtly wrong for a 20% win.
- Carrying the flag is local, provable, and testable with the suites that already
  exist.

## Outcome (0.2.32): implemented, measured, reverted

The design above was implemented as one slice. It did not measure, so no code
from it is kept. Both halves of that — why the design cannot work as written, and
what the measurement said about the sound remainder — are worth more than the
attempt was.

### Rule 1 is unsound: a node's rewrite can undo a child's change

"Trust the children: if any child changed, the rebuilt node differs from its
input" is false. The node's own rewrite can put the original back.

The witness is `1/x - x/(1 + x^2)`, which `Integrate[1/(x^3 + x), x]` produces.
Its child `-x/(1 + x^2)` rewrites (to a form that prints identically but is not
structurally equal — the difference is in an inner ordering), and the `Plus` that
owns it then rebuilds a value structurally equal to its own input. Under rule 1
that node reports "changed" on every pass, forever: the first implementation ran
200 000 passes and stopped only on `$IterationLimit::itlim`. `Integrate[x^3/(1 +
x^2)^2, x]` does the same thing.

The visible half of the same bug was caught earlier, by the golden suite:
`Steps[1 + 2 3]` lost its final `eval` form because the top-level loop stopped one
pass early — `{Given: 1 + 2 3, eval: 6, eval: 7}` became `{…, eval: 6}`. That is
what a "changed" flag that is wrong in the *other* direction looks like.

And the first implementation was **21% faster** on `Expand[(x + y + z)^40]`
(4.2 s → 3.3 s, three runs each side): faster exactly because it ended a pass
early. A number that good from a change that breaks the fixpoint is a warning,
not a result — which is why the acceptance bar in this note puts the suites
before the stopwatch.

### The sound version does not measure

What survives rule 1's failure is narrower. Skip the comparison only when the
dispatch returned *exactly* the node rebuilt from its evaluated arguments: then
the node's result is its children's result, and the children's flags decide it.
Anything else — the rule or built-in produced a different expression — still
compares with the input, because a rewrite can be undone.

For the children's flags to be usable as a proof they must be exact, so
`eval_tracked` reports "the returned value differs from the expression passed
in": free when the first pass already reports "unchanged" (that pass just made the
comparison), and one comparison when a subtree took several passes.

That condition held for **96.6%** of dispatch results on `Expand[(x + y + z)^40]`
(2 778 579 of 2 877 628), so the shortcut was exercised, not dodged. Measured
against the same binary built from `v0.2.31`, three runs each way:

| | `Expand[(x + y + z)^40]` | `Nest[f, x, 1600]` |
|---|---|---|
| instructions before | 36 962 850 747 · 36 962 911 976 · 36 962 850 747 | 2 720 459 868 · 2 720 456 102 · 2 720 458 357 |
| instructions after | 37 236 122 528 · 37 236 089 861 · 37 236 243 273 | 2 734 585 621 · 2 734 586 884 · 2 734 609 623 |
| | **+0.74%** | **+0.52%** |
| CPU time, median of 3 each way | neutral | neutral (≈ +2%) |

Instruction counts are the measurement that can be trusted on this machine: three
runs of one binary agreed to within 0.001%, while CPU and wall time of the *same*
binary moved between 4.13 s and 5.09 s across runs (a loaded desktop; `perf stat
-e instructions:u` is what settled it, and `scripts/dev/bench.sh` prints both
CPU and wall time for the two workloads with the note's three runs).

So the comparison being removed was already cheap — `same_q_shallow` answers the
common case with pointer equality, and the O(size) deep comparison only runs when
a result really is equal to its input — and the evidence required to skip it costs
at least as much as it saves. The comparison was never the cost.

### What the cost actually is

`perf` (self time, release build with frame pointers) on the two workloads:

| share | `Expand[(x + y + z)^40]` | `Nest[f, x, 1600]` |
|---|---|---|
| allocator (`malloc`, `free`, `Rc::drop_slow`, `Vec` growth) | 38.7% | 51.5% |
| hashing (`Expr::hash`, the `Plus`/`Times` term index) | 8.1% | 5.0% |
| evaluator loop (`eval`, `eval_node`, `eval_args`) | 8.9% | 31.1% |
| sort keys (`factor_key` on every comparison) | 3.0% | — |
| `same_q` | 2.7% | not measurable |
| case-insensitive name hint (memoised) | — | 3.4% |

The premise of this note — that the remaining cost is the O(size) comparison — is
falsified by that table. What is left is the cost of *building* expressions: one
small allocation per node per pass, and the hashing that 0.2.17 introduced to kill
the O(k²) term merge. That is the ground hash-consing would attack. It is still
rejected, but for the honest reason: it would be a change to the representation
with a memory and lifetime cost of its own, for a workload nobody has complained
about, and this slice shows how easily a plausible evaluation redesign buys speed
by changing meaning.

`1/x - x/(1 + x^2)` and `Integrate[1/(x^3 + x), x]` are pinned in
`eval::tests::a_rewrite_the_node_undoes_still_terminates` so the next attempt at
this design fails in a second instead of in a 200 000-pass loop.
