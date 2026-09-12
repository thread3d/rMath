# Special functions and the integrals that need them
# (∫ sin(x)/x dx = SinIntegral[x], etc.)
Integrate[Sin[x]/x, x]
=> SinIntegral[x]
Integrate[Sin[2 x]/x, x]
=> SinIntegral[2 x]
Integrate[Sin[x]/(2 x), x]
=> 1/2 SinIntegral[x]
Integrate[Cos[x]/x, x]
=> CosIntegral[x]
Integrate[Exp[x]/x, x]
=> ExpIntegralEi[x]
Integrate[1/Log[x], x]
=> LogIntegral[x]
Integrate[Exp[-x^2], x]
=> 1/2 Erf[x] Sqrt[Pi]
# no rule: stays unevaluated rather than guessing
Integrate[Sin[x + 1]/x, x]
=> Integrate[Sin[1 + x]/x, x]
D[SinIntegral[x], x]
=> Sin[x]/x
D[Erf[x], x]
=> 2 Exp[-x^2]/Sqrt[Pi]
Erf[0]
=> 0
Erf[Infinity]
=> 1
SinIntegral[Infinity]
=> 1/2 Pi
N[Erf[1]]
=> 0.842701
N[SinIntegral[1]]
=> 0.946083
# half powers print as radicals, other rational exponents are parenthesised
Sqrt[2]
=> Sqrt[2]
x^(1/3)
=> x^(1/3)
