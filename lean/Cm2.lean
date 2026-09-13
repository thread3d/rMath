import Mathlib

example (x : ℝ) : deriv (((fun x : ℝ => x ^ (2 : ℕ)) + (fun x : ℝ => (2 : ℝ) * (id x))) + (fun _ : ℝ => (1 : ℝ))) x = ((2 : ℝ) + ((2 : ℝ) * x)) := by
  have h := (((((hasDerivAt_pow 2 x).add (((hasDerivAt_id x).const_mul ((2 : ℝ)))))).add (hasDerivAt_const ((1 : ℝ)) x)))
  simpa using h.deriv
