# Integration rules added in the hardening pass: arc forms, integration by
# parts, x^n Log[x], and partial fractions (with the Apart built-in).

Integrate[1/(1+x^2), x]
=> ArcTan[x]
Integrate[1/(4+x^2), x]
=> 1/2 ArcTan[1/2 x]
Integrate[1/Sqrt[1-x^2], x]
=> ArcSin[x]
Integrate[1/Sqrt[4-x^2], x]
=> ArcSin[1/2 x]

Integrate[x Sin[x], x]
=> -x Cos[x] + Sin[x]
Integrate[x Cos[x], x]
=> x Sin[x] + Cos[x]
Integrate[x Exp[x], x]
=> -Exp[x] + Exp[x] x
Integrate[x^2 Exp[x], x]
=> Exp[x] x^2 - 2 (-Exp[x] + Exp[x] x)
Integrate[2 x Sin[x], x]
=> 2 (-x Cos[x] + Sin[x])

Integrate[x Log[x], x]
=> -1/4 x^2 + 1/2 x^2 Log[x]
Integrate[x^2 Log[x], x]
=> -1/9 x^3 + 1/3 x^3 Log[x]

Apart[1/(x^2-1), x]
=> -1/2/(1 + x) + 1/2/(-1 + x)
Apart[1/(x(x+1)), x]
=> 1/x - 1/(1 + x)
Apart[(x^2+1)/(x-1), x]
=> 1 + x + 2/(-1 + x)
Integrate[1/(x^2-1), x]
=> -1/2 Log[1 + x] + 1/2 Log[-1 + x]
Integrate[1/(x(x+1)), x]
=> Log[x] - Log[1 + x]
Integrate[1/((x-1)(x-2)), x]
=> Log[-2 + x] - Log[-1 + x]

# Powers of Sin/Cos by the reduction formula (regression: every trig power
# stayed unevaluated, including the textbook Integrate[Sin[x]^2, x]).
Integrate[Sin[x]^2, x]
=> 1/2 x - 1/2 Cos[x] Sin[x]
Integrate[Cos[x]^2, x]
=> 1/2 x + 1/2 Cos[x] Sin[x]
Integrate[Sin[x]^3, x]
=> -2/3 Cos[x] - 1/3 Cos[x] Sin[x]^2
Integrate[Cos[x]^4, x]
=> 3/8 x + 1/4 Cos[x]^3 Sin[x] + 3/8 Cos[x] Sin[x]
Integrate[Sin[2 x]^2, x]
=> 1/2 x - 1/4 Cos[2 x] Sin[2 x]
Integrate[Sin[3 x + 1]^2, x]
=> 1/6 + 1/2 x - 1/6 Cos[1 + 3 x] Sin[1 + 3 x]
Integrate[Sin[x]^2, {x, 0, Pi}]
=> 1/2 Pi
Integrate[Cos[x]^3, {x, 0, Pi/2}]
=> 2/3
# A product of trig powers with one odd exponent goes through the substitution
# t = sin u or t = cos u; both exponents even needs product-to-sum, still open.
Integrate[Sin[x]^2 Cos[x]^3, x]
=> -1/5 Sin[x]^5 + 1/3 Sin[x]^3
Integrate[Sin[x]^3 Cos[x]^2, x]
=> -1/3 Cos[x]^3 + 1/5 Cos[x]^5
Integrate[Sin[x] Cos[x], x]
=> 1/2 Sin[x]^2
Integrate[Sin[2 x]^3 Cos[2 x], x]
=> 1/8 Sin[2 x]^4
# Both exponents even: the reduction lowers n by two, which is product-to-sum in
# disguise (the answer stays in terms of sin u cos u rather than sin 2u).
Integrate[Sin[x]^2 Cos[x]^2, x]
=> 1/8 x - 1/8 Cos[x] Sin[x] + 1/4 Cos[x] Sin[x]^3
Integrate[Sin[x]^2 Cos[x]^4, x]
=> 1/16 x - 1/16 Cos[x] Sin[x] + 1/8 Cos[x] Sin[x]^3 + 1/6 Cos[x]^3 Sin[x]^3
Integrate[Sin[x]^4 Cos[x]^4, x]
=> 3/128 x - 3/128 Cos[x] Sin[x] - 1/64 Cos[x] Sin[x]^3 + 1/16 Cos[x] Sin[x]^5 + 1/8 Cos[x]^3 Sin[x]^5
Integrate[Sin[x]^2 Cos[x]^2, {x, 0, Pi}]
=> 1/8 Pi

# Repeated linear factors in the denominator: the cover-up rule generalises to a
# power series about the root, so 1/(x^2 (1+x)) splits instead of staying put.
Apart[1/(x^2 (1 + x)), x]
=> 1/x^2 + 1/(1 + x) - 1/x
Apart[1/((x - 1)^2 (x + 1)), x]
=> -1/4/(-1 + x) + 1/4/(1 + x) + 1/2 1/(-1 + x)^2
Apart[(x + 1)/(x^2 (x - 1)), x]
=> -2/x - 1/x^2 + 2/(-1 + x)
Integrate[1/(x^2 (1 + x)), x]
=> Log[1 + x] - Log[x] - 1/x
