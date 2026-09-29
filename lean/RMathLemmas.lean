import Mathlib

/-!
Support lemmas and tactics for the rMath bridge.

The bridge generates one Lean file per claim and runs it with
`lake env lean`. This file gives that generated file a small preamble:

* `rmath_radical` — closes goals with nested real square roots (the kernel's
  `Sqrt`), which `norm_num`/`ring`/`positivity` cannot discharge.
* `@[simp]`/`@[positivity]` facts for the kernel's `Real.*` function set.

It is inlined by `rmathpy/lean.py` (the `import Mathlib` line is stripped and the
rest prepended), so it needs no separate `lake build` step.
-/

open Lean Elab Tactic Meta

/-- Every argument `t` of a `Real.sqrt t` subterm of `e`. -/
partial def rmathSqrtArgs (e : Expr) (acc : Array Expr) : Array Expr :=
  match e with
  | .app (.const ``Real.sqrt _) a => rmathSqrtArgs a (acc.push a)
  | .app f a => rmathSqrtArgs a (rmathSqrtArgs f acc)
  | .lam _ d b _ => rmathSqrtArgs b (rmathSqrtArgs d acc)
  | .forallE _ d b _ => rmathSqrtArgs b (rmathSqrtArgs d acc)
  | .letE _ t v b _ => rmathSqrtArgs b (rmathSqrtArgs v (rmathSqrtArgs t acc))
  | .mdata _ b => rmathSqrtArgs b acc
  | .proj _ _ b => rmathSqrtArgs b acc
  | _ => acc

/-- The number of `Real.sqrt` applications inside `e`. -/
partial def rmathSqrtDepth (e : Expr) : Nat :=
  match e with
  | .app (.const ``Real.sqrt _) a => 1 + rmathSqrtDepth a
  | .app f a => rmathSqrtDepth f + rmathSqrtDepth a
  | .lam _ d b _ => rmathSqrtDepth d + rmathSqrtDepth b
  | .forallE _ d b _ => rmathSqrtDepth d + rmathSqrtDepth b
  | .letE _ t v b _ => rmathSqrtDepth t + rmathSqrtDepth v + rmathSqrtDepth b
  | .mdata _ b => rmathSqrtDepth b
  | .proj _ _ b => rmathSqrtDepth b
  | _ => 0

/-- Close a goal with nested real square roots.

Bottom-up over the radicals in the goal: for each radicand `t` (innermost
first) prove `0 ≤ t`, record `(√t)² = t` and `0 ≤ √t`, then finish with
`nlinarith`. `√((√5-1)/2)` needs exactly this: `0 ≤ (√5-1)/2` follows from
`(√5)² = 5` together with `√5 ≥ 0`, which `positivity` alone cannot see. Any
radical whose sign is genuinely unprovable makes the whole tactic fail, so a
verdict is never invented.
-/
elab "rmath_radical" : tactic => do
  let g ← getMainGoal
  let ty ← instantiateMVars (← g.getType)
  let mut seen : Array Expr := #[]
  for a in rmathSqrtArgs ty #[] do
    unless seen.any (fun b => b == a) do seen := seen.push a
  let args := seen.qsort (fun a b => rmathSqrtDepth a < rmathSqrtDepth b)
  if args.isEmpty then
    evalTactic (← `(tactic| first | norm_num | ring | ring_nf | positivity | nlinarith))
    return
  let mut hyps : Array (TSyntax `term) := #[]
  let mut i := 0
  for a in args do
    i := i + 1
    let aS ← Lean.Elab.Term.exprToSyntax a
    let h := mkIdent (Name.mkSimple s!"rmath_h{i}")
    let nn := mkIdent (Name.mkSimple s!"rmath_nn{i}")
    let nnT : TSyntax `term := ⟨nn.raw⟩
    let sq := mkIdent (Name.mkSimple s!"rmath_sq{i}")
    let sqT : TSyntax `term := ⟨sq.raw⟩
    evalTactic (← `(tactic| have $h : (0 : ℝ) ≤ $aS := by
      first | positivity | norm_num | nlinarith [$[$hyps],*]))
    evalTactic (← `(tactic| have $nn : (0 : ℝ) ≤ Real.sqrt ($aS) := Real.sqrt_nonneg _))
    hyps := hyps.push nnT
    evalTactic (← `(tactic| have $sq : (Real.sqrt ($aS)) ^ 2 = $aS := Real.sq_sqrt $h))
    hyps := hyps.push sqT
  -- A Solve claim is a conjunction of root equalities; `and_intros` splits every
  -- `∧` (and is a no-op on a single goal), then each part is closed by `nlinarith`
  -- with the facts gathered above.
  evalTactic (← `(tactic| and_intros <;> nlinarith [$[$hyps],*]))

/-- `(√x)² = x` as a `simp` lemma; the side condition becomes a `positivity`
goal, which `simp` discharges for literal/obviously-nonnegative arguments. -/
@[simp] theorem rmath_sqrt_sq {x : ℝ} (h : 0 ≤ x) : Real.sqrt x ^ 2 = x :=
  Real.sq_sqrt h

/-- `rMath`-friendly names for the positivity facts the kernel's output needs. -/
@[simp] theorem rmath_sqrt_nonneg (x : ℝ) : 0 ≤ Real.sqrt x := Real.sqrt_nonneg x

/-- A `positivity` fact for `Real.exp`, matching the kernel's `Exp`. -/
theorem rmath_exp_pos (x : ℝ) : 0 < Real.exp x := Real.exp_pos x
