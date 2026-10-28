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
