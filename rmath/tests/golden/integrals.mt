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
