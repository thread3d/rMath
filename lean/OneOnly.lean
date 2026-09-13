import Mathlib

example (x : ℝ) : 3 * x ^ 2 + Real.cos x = Real.cos x + 3 * x ^ 2 := by ring_nf
