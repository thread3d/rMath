import Mathlib

example : Real.sin ((Real.pi / (6 : ℝ))) = ((1 : ℝ) / (2 : ℝ)) := by
  solve | norm_num | ring | ring_nf | positivity | linarith | nlinarith | omega | simp | decide | rfl
