import Mathlib

example (x : ℝ) : 3 * x ^ 2 + Real.cos x = Real.cos x + 3 * x ^ 2 := by ring_nf

example : deriv id x = (1 : ℝ) := by
  have h := (hasDerivAt_id x)
  first | simpa using h.deriv | (rw [h.deriv]; ring_nf)

example : deriv ((fun x : ℝ => x ^ (3 : ℕ)) + (fun x : ℝ => Real.sin (id x))) x = (Real.cos (x) + ((3 : ℝ) * (x ^ (2 : ℕ)))) := by
  have h := (((hasDerivAt_pow 3 x).add (((hasDerivAt_id x).sin))))
  first | simpa using h.deriv | (rw [h.deriv]; ring_nf) | (rw [h.deriv]; ring) | (rw [h.deriv]; norm_num)

example : deriv (fun _ : ℝ => (7 : ℝ)) x = (0 : ℝ) := by
  have h := (hasDerivAt_const (7 : ℝ) x)
  first | simpa using h.deriv | (rw [h.deriv]; ring_nf) | (rw [h.deriv]; ring) | (rw [h.deriv]; norm_num)
