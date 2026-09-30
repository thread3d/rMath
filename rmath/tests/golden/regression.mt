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
=> -2 x + 2 x^2 - x^3 + 5/4 x^4 - 6/5 x^5

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

# --- Plus terms follow Mathematica's polynomial order ---
# Constants first, then monomials by exponent vector from the last variable to
# the first (the reverse-lexicographic order the Wolfram docs describe). Terms
# used to sort alphabetically by head, so `x^2` came before `2 x` and a bare
# symbol outranked a Power, printing e.g. `1 + x^2 + 2 x`.
Expand[(x+3)(x+2)]
=> 6 + 5 x + x^2
Expand[(1+x)^5]
=> 1 + 5 x + 10 x^2 + 10 x^3 + 5 x^4 + x^5
Expand[(x+y)^2 (x-y)^2]
=> x^4 - 2 x^2 y^2 + y^4
Expand[(x+y)^5 - (x^5 + y^5)]
=> 5 x^4 y + 10 x^3 y^2 + 10 x^2 y^3 + 5 x y^4
Expand[(1+x+y)(2-x)^3]
=> 8 - 4 x - 6 x^2 + 5 x^3 - x^4 + 8 y - 12 x y + 6 x^2 y - x^3 y
x^2 + y
=> x^2 + y
1 + x + Sin[x]
=> 1 + x + Sin[x]
D[x^3 + Sin[x], x]
=> 3 x^2 + Cos[x]

# --- Coefficient: unexpanded input, symbolic coefficients, monomial forms ---
# It used to need an explicitly expanded polynomial with numeric coefficients,
# so `Coefficient[a x + b y + c, x]` and `Coefficient[(x+1)^5, x^3]` stayed
# unevaluated. Mathematica's exact-exponent rule is kept: x^2 is not part of x.
Coefficient[(x + 1)^3, x, 2]
=> 3
Coefficient[a x + b y + c, x]
=> a
Coefficient[a x^3 + b x^2 + c x + d, x, 2]
=> b
Coefficient[(x + 2)^2 + (x + 3)^3, x, 0]
=> 31
Coefficient[(x + y)^4, x y^3]
=> 4
Coefficient[(x + y)(x + 2 y)(3 x + 4 y + 5), x y^2]
=> 18
Coefficient[x^2, x]
=> 0
Coefficient[x y, x]
=> y

# --- Times factors: coefficient first, then symbolic bases by name ---
# A number, a constant symbol, a numeric-valued factor (Sqrt[2]) or E^x all
# precede the variables; functions follow powers. Previously a bare symbol
# outranked every node, printing `x Exp[x]` and `Log[x] x^2`.
Sqrt[2] x
=> Sqrt[2] x
2 Pi x
=> 2 Pi x
Exp[x] x^2
=> Exp[x] x^2
x^2 Log[x]
=> x^2 Log[x]
x Cos[x]
=> x Cos[x]
Sqrt[Pi] Erf[x]
=> Sqrt[Pi] Erf[x]
5 x^4 y
=> 5 x^4 y

# --- Sort uses Mathematica's canonical order ---
# Numbers compare by value (exact and machine together); `Sqrt[2]` is a
# numeric-valued expression, symbols/monomials come next by base and degree,
# sums by length. Previously Sort used the internal Orderless order, which
# grouped numbers by exactness and put symbols before every node.
Sort[{y, x^2, x + y, y^3}]
=> {x^2, y, y^3, x + y}
Sort[{Pi, E, 2, 3, 1, Sqrt[2]}]
=> {1, 2, 3, Sqrt[2], E, Pi}
Sort[{5.6, 5, 2.3, -1, 4/3, 1}]
=> {-1, 1, 4/3, 2.3, 5, 5.6}
Sort[{a + b + c, a + b, a + c}]
=> {a + b, a + c, a + b + c}
Ordering[{y, x^2, x + y, y^3}]
=> {2, 1, 4, 3}

# --- Part with a list of indices (the `list[[Ordering[list]]]` idiom) ---
{a, b, c}[[{3, 1, 2}]]
=> {c, a, b}
{a, b, c}[[{-1, 1}]]
=> {c, a}
{{a, b}, {c, d}}[[{2, 1}, 2]]
=> {d, b}
{y, x^2, x + y, y^3}[[Ordering[{y, x^2, x + y, y^3}]]]
=> {x^2, y, y^3, x + y}
