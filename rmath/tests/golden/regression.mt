# Regression tests for hardening work: the central arity guard, complex
# negative powers, zero/infinity arithmetic, symbolic exponents in Times, the
# Integrate power rule, the D fallback, the Series/Limit off-by-one, negative
# MatrixPower and Round-ties-to-even. Every case here used to crash, return a
# wrong value, or return a malformed string.

# --- central arity guard: malformed calls stay unevaluated, no panic ---
First[]
=> First[]
Length[]
=> Length[]
ToString[]
=> ToString[]
Names[]
=> Names[]
Take[x]
=> Take[x]
StringTake["ab"]
=> StringTake["ab"]

# --- complex negative powers (1/I used to overflow the stack) ---
1/I
=> -I
Re[1/(1+I)]
=> 1/2
Im[1/(1+I)]
=> -1/2
Re[(2+3 I)^-1]
=> 2/13
Im[(2+3 I)^-1]
=> -3/13

# --- zero and infinity arithmetic (used to return 0, 1, ...) ---
0/0
=> Indeterminate
0*Infinity
=> Indeterminate
0*ComplexInfinity
=> Indeterminate
Infinity/Infinity
=> Indeterminate
Infinity^0
=> Indeterminate
Infinity - Infinity
=> Indeterminate
1/Infinity
=> 0
Infinity^-1
=> 0
Infinity^2
=> Infinity

# --- Times must keep symbolic exponents (x^n y used to collapse to y) ---
a^m a^n
=> a^(m + n)
x^n x
=> x^(1 + n)
x^n/x
=> x^(-1 + n)
x^-1 y
=> y/x
x x^-1
=> 1

# --- Integrate power rule (x^n and Sqrt[x] used to return Log[x]) ---
Integrate[x^n, x]
=> x^(1 + n)/(1 + n)
Integrate[Sqrt[x], x]
=> 2/3 x^(3/2)
Integrate[x^-2, x]
=> -1/x
Integrate[1/(2 x + 1), x]
=> 1/2 Log[1 + 2 x]

# --- D: an unknown function gets a Derivative, not the function itself ---
D[f[x], x]
=> Derivative[1][f][x]
D[f[x] g[x], x]
=> f[x] Derivative[1][g][x] + g[x] Derivative[1][f][x]
D[f[x^2], x]
=> 2 x Derivative[1][f][x^2]
D[f[2], x]
=> 0
D[Abs[x], x]
=> Derivative[1][Abs][x]

# --- Series/Normal: the coefficient list is offset by nmin ---
Normal[Series[Sin[x], {x, 0, 5}]]
=> x - 1/6 x^3 + 1/120 x^5
Normal[Series[Sin[x], {x, 0, 2}]]
=> x

# --- Limit: l'Hopital for 0/0 forms, and no invented answers ---
Limit[Sin[x]/x, x -> 0]
=> 1
Limit[(x^2-1)/(x-1), x -> 1]
=> 2
Limit[(1-Cos[x])/x^2, x -> 0]
=> 1/2
Limit[Tan[x]/x, x -> 0]
=> 1
Limit[1/x, x -> Infinity]
=> 0
Limit[1/x, x -> 0]
=> ComplexInfinity

# --- MatrixPower with a negative exponent (used to return the identity) ---
MatrixPower[{{1,2},{3,4}}, -1]
=> {{-2, 1}, {3/2, -1/2}}
MatrixPower[{{1,1},{0,1}}, -2]
=> {{1, -2}, {0, 1}}

# --- Round ties to the nearest even integer ---
Round[1/2]
=> 0
Round[5/2]
=> 2
Round[-1/2]
=> 0
Round[7/2]
=> 4

# --- OutputForm: a negative numerator with a denominator ---
-1/x
=> -1/x

# --- Factor: the content is applied once, not multiplied in twice ---
# (bug report 20260927-163001: Factor[2x^2-2] returned 4(-1+x)(1+x))
Factor[2 x^2 - 2]
=> 2 (-1 + x) (1 + x)
Factor[6 x^2 - 6]
=> 6 (-1 + x) (1 + x)
Factor[2 x^2 + 4 x + 2]
=> 2 (1 + x)^2
Factor[2 x^3 - 2 x]
=> 2 x (-1 + x) (1 + x)
Factor[6 x^2 - 6 x]
=> 6 x (-1 + x)
Factor[-2 x^2 + 2]
=> -2 (-1 + x) (1 + x)
# A unit content must stay untouched.
Factor[4 x^2 - 9]
=> 4 (-3/2 + x) (3/2 + x)

# --- Orderless canonical order must be antisymmetric ---
# canon_cmp compared Int/Rat as `Int.cmp(Rat)` in both directions, so the sort
# oscillated and the evaluator looped until $IterationLimit. It surfaced as
# Integrate hanging on some polynomials; both of these now return promptly.
- x^3 + 5/4 x^4
=> -x^3 + 5/4 x^4
Integrate[-3 x^2 + 5 x^3, x]
=> -x^3 + 5/4 x^4
Integrate[-2 + 4 x - 3 x^2 + 5 x^3 - 6 x^4, x]
=> -2 x - 6/5 x^5 - x^3 + 5/4 x^4 + 2 x^2

# --- Cancel must actually cancel (it used to be an alias for Together) ---
Cancel[(x^2 - 9)/(x - 3)]
=> 3 + x
Cancel[(2 x^2 - 2)/(x - 1)]
=> 2 + 2 x
Cancel[(x^2 - 1)/(x^2 + 2 x + 1)]
=> (-1 + x)/(1 + x)
Cancel[(6 x^2 - 6)/(2 x - 2)]
=> 3 + 3 x

# --- Apart of a polynomial is the identity, not an unevaluated head ---
Apart[x^2 + 1, x]
=> 1 + x^2
