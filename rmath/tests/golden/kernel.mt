# Regression tests for bugs found while generating the demo file

# parser: a compound expression is a legal function argument
f[a; b]
=> f[b]
# parser: a pure function made with & can be applied directly
#^2 &[7]
=> 49
# Return inside a function body returns from the function, not the session
retf[x_] := (If[x > 0, Return[positive]]; nonpositive)
=> 
retf[5]
=> positive
retf[-1]
=> nonpositive
# Reap holds its argument, so a compound expression inside it works
Reap[Sow[7]; Sow[8]]
=> {8, {{7, 8}}}
# Log[E] and Log[E^n]
Log[E]
=> 1
Log[E^3]
=> 3
# Gamma: exact values and numeric evaluation (Lanczos coefficients were wrong)
Gamma[5]
=> 24
Gamma[1]
=> 1
Gamma[1/2]
=> Sqrt[Pi]
Gamma[3/2]
=> 1/2 Sqrt[Pi]
N[Gamma[5]]
=> 24.0
# Range with non-integer steps
Range[0, 1, 1/4]
=> {0, 1/4, 1/2, 3/4, 1}
Range[5, 1, -2]
=> {5, 3, 1}
Range[0, 1, 0.25]
=> {0., 0.25, 0.5, 0.75, 1.0}
