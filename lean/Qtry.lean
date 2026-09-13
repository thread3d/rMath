import Mathlib

example (x : ℝ) (h1 : x ≠ 0) : deriv ((fun x : ℝ => Real.sin (id x)) / id) x
    = ((Real.cos x / x) - ((1 : ℝ) / (x ^ 2)) * Real.sin x) := by
  have h := (((hasDerivAt_id x).sin).div (hasDerivAt_id x) h1)
  rw [h.deriv]
  simp only [id_eq]
  field_simp
  ring
