import Mathlib

example (x : ℝ) : deriv (fun x => (2 : ℝ) * (id x)) x = (2 : ℝ) := by
  have h := (hasDerivAt_id x).const_mul (2 : ℝ)
  simpa using h.deriv
