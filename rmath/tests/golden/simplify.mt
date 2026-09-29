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
