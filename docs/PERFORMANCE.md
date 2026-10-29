# Evaluator performance: the change-tracking design note

The one performance item left. Written before any code, because the last two
attempts were coded first and measured neutral — the record of what was tried
belongs here rather than in a commit message nobody will find again.

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
