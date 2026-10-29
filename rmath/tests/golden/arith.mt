# arithmetic and exact numerics
1 + 2 * 3
=> 7
1/2 + 1/3
=> 5/6
2^100
=> 1267650600228229401496703205376
N[Pi]
=> 3.14159
N[1/3]
=> 0.333333
2^(-3)
=> 1/8
Sqrt[16]
=> 4
Sqrt[2] + Sqrt[2]
=> 2 Sqrt[2]
5 - 7
=> -2
(-3)^2
=> 9
x + x + 2x
=> 4 x
2x + 3x
=> 5 x
Abs[-5]
=> 5
Mod[10, 3]
=> 1
GCD[24, 36]
=> 12
10!
=> 3628800
Floor[2.7]
=> 2.0
# Mathematica rounds ties to the nearest even integer (Round[1/2] = 0).
Round[1/2]
=> 0

# Chop must return the argument itself, not Chop[<argument>] (regression: every
# real that was not small enough to become 0 came back wrapped in its own
# head, so Chop[0.1] printed as Chop[0.1] and Chop[1.0*^-7] as Chop[1.0*^-7]),
# and it threads through lists and into arguments as Mathematica's does.
Chop[0.1]
=> 0.1
Chop[0.0000001]
=> 1.0*^-7
Chop[1.0*^-12]
=> 0
Chop[{1.0*^-12, 2.5}]
=> {0, 2.5}
Chop[1.0*^-12 + 2.0 I]
=> 2.0 I
Chop[0.01, 0.1]
=> 0
Chop[1/10000000]
=> 1/10000000
Chop[x]
=> x

# Exact values at multiples of Pi/6, Pi/4, Pi/3 and Pi/2 (regression: only
# integer multiples of Pi were exact, so Sin[Pi/6] stayed Sin[1/6 Pi] and
# definite integrals over [0, Pi/2] did not close).
Sin[Pi/6]
=> 1/2
Cos[Pi/3]
=> 1/2
Sin[Pi/4]
=> 1/2 Sqrt[2]
Tan[Pi/4]
=> 1
Cot[Pi/4]
=> 1
Csc[Pi/2]
=> 1
Sec[Pi/3]
=> 2
Sin[Pi/2]
=> 1
Cos[Pi/2]
=> 0
Sin[5 Pi/6]
=> 1/2
Sin[-Pi/6]
=> -1/2
Cos[2 Pi/3]
=> -1/2
Tan[Pi/3]
=> Sqrt[3]
Sin[Pi]
=> 0
Cos[Pi]
=> -1
Tan[Pi/2]
=> ComplexInfinity
Cot[0]
=> ComplexInfinity
Sin[Pi/5]
=> Sin[1/5 Pi]

# Exact values outside trigonometry: 0 and 1 of the hyperbolics, the
# inverse functions at 0/+/-1/+/-1/2, Exp[1] = E.
Exp[1]
=> E
Exp[Log[x]]
=> x
Sinh[0]
=> 0
Cosh[0]
=> 1
ArcSin[1]
=> 1/2 Pi
ArcSin[1/2]
=> 1/6 Pi
ArcCos[1]
=> 0
ArcTan[1]
=> 1/4 Pi
Integrate[Cos[x], {x, 0, Pi/2}]
=> 1
Integrate[Sin[x], {x, 0, Pi/2}]
=> 1

# GCD and LCM must work on arbitrary-size integers (regression: both read
# their arguments through a machine-sized conversion, so GCD[2^64, 2^70]
# stayed unevaluated while every small case worked).
GCD[2^64, 2^70]
=> 18446744073709551616
GCD[2^200 - 1, 2^100 - 1]
=> 1267650600228229401496703205375
LCM[6, 10]
=> 30
LCM[2^70, 3]
=> 3541774862152233910272
GCD[-12, 18]
=> 6
GCD[12.0, 18]
=> 6
GCD[x, 2]
=> GCD[x, 2]

# Max, Min and Binomial were missing entirely (Max[1, 3, 2] stayed unevaluated).
Max[1, 3, 2]
=> 3
Min[1, 3, 2]
=> 1
Max[1/2, 1/3]
=> 1/2
Max[1.5, 2]
=> 2
Max[x, 1]
=> Max[x, 1]
Max[]
=> -Infinity
Min[]
=> Infinity
Binomial[5, 2]
=> 10
Binomial[10, 5]
=> 252
Binomial[5, 7]
=> 0
Binomial[x, 2]
=> Binomial[x, 2]
