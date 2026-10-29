# Simplify / FullSimplify with assumptions.

$Assumptions
=> True
Simplify[Sqrt[x^2]]
=> Abs[x]
Simplify[Sqrt[x^2], x > 0]
=> x
Simplify[Sqrt[x^2], x < 0]
=> -x
Assuming[x > 0, Simplify[Sqrt[x^2]]]
=> x
Simplify[Abs[x], x >= 0]
=> x
Simplify[Log[Exp[x]]]
=> x
Simplify[Exp[Log[x]]]
=> x
Simplify[Sin[x]^2 + Cos[x]^2]
=> 1
Simplify[(x^2)^3]
=> x^6

# Rational cancellation via the polynomial GCD.
Simplify[(x^2-1)/(x-1)]
=> 1 + x
Simplify[(x^2+2x+1)/(x+1)]
=> 1 + x
Simplify[(x^3-1)/(x-1)]
=> 1 + x + x^2
Simplify[(x^2-1)/(x-1) + 1]
=> 2 + x
FullSimplify[(x^2+2x+1)/(x+1)]
=> 1 + x

# Ordinary folding is unchanged.
Simplify[1 + 1]
=> 2
Simplify[2 x + 3 x]
=> 5 x

# Cancel now reduces a multivariate fraction by its common monomial factor and
# numeric content, and returns what it managed even when the univariate GCD step
# cannot run (regression: `x^2 y/(x y^2)` came back unchanged, and a partially
# cancelled fraction was thrown away in favour of the original).
Cancel[(x^2 y)/(x y^2)]
=> x/y
Cancel[(x^4 y^2)/(x^2 y^5)]
=> x^2/y^3
Cancel[(x^2 y + x y^2)/(x y)]
=> x + y
Cancel[(2 x y)/(4 x)]
=> 1/2 y
# A symbolic coefficient is just another variable to the multivariate GCD, which
# now cancels the shared factor `1 + x`.
Cancel[(a x + a)/(x + 1)]
=> a
Cancel[(x^2 - y^2)/(x - y)]
=> x + y
Cancel[(x^3 + y^3)/(x + y)]
=> x^2 - x y + y^2
Cancel[(x^2 y + x y^2)/(x + y)]
=> x y
Cancel[(x^2 + y)/(x^3 + y)]
=> (x^2 + y)/(x^3 + y)
PolynomialGCD[x^2 - y^2, x - y]
=> x - y
