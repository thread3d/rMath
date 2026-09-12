# algebra, calculus, matrices
Expand[(1+x)^2]
=> 1 + x^2 + 2 x
Expand[(a+b)(c+d)]
=> a c + a d + b c + b d
Factor[x^2 + 5x + 6]
=> (2 + x) (3 + x)
Factor[x^2 - 4]
=> (-2 + x) (2 + x)
Collect[x + x^2 + 2x^2 + 3, x]
=> 3 + x + 3 x^2
Coefficient[x^3 - 2x^2 + 5, x, 2]
=> -2
Together[1/x + 1/y]
=> (x + y)/(x y)
D[x^3, x]
=> 3 x^2
D[Sin[x]^2, x]
=> 2 Cos[x] Sin[x]
D[x^3, {x, 2}]
=> 6 x
D[Exp[2x], x]
=> 2 Exp[2 x]
Integrate[x^2, x]
=> 1/3 x^3
Integrate[Sin[x], x]
=> -Cos[x]
Integrate[1/x, x]
=> Log[x]
Integrate[x^2, {x, 0, 1}]
=> 1/3
Normal[Series[Cos[x], {x, 0, 2}]]
=> 1 - 1/2 x^2
Det[{{2, 1}, {1, 3}}]
=> 5
Inverse[{{2, 1}, {1, 3}}]
=> {{3/5, -1/5}, {-1/5, 2/5}}
Dot[{{1, 2}, {3, 4}}, {5, 6}]
=> {23, 34}
Transpose[{{1, 2}, {3, 4}}]
=> {{1, 3}, {2, 4}}
LinearSolve[{{2, 1}, {1, 3}}, {5, 10}]
=> {1, 3}
RowReduce[{{1, 2, 3}, {4, 5, 6}}]
=> {{1, 0, -1}, {0, 1, 2}}
Solve[x^2 + 5x + 6 == 0, x]
=> {{x -> -2}, {x -> -3}}
Solve[2x + 3 == 11, x]
=> {{x -> 4}}
Solve[{x + y == 5, x - y == 1}, {x, y}]
=> {{x -> 3, y -> 2}}
NSolve[x^3 - 6x^2 + 11x - 6 == 0, x]
=> {1.0, 2.0, 3.0}
MatrixPower[{{1, 1}, {0, 1}}, 3]
=> {{1, 3}, {0, 1}}
MatrixRank[{{1, 2}, {2, 4}}]
=> 1
D[Log[x], x]
=> 1/x
Series[Cos[x], {x, 0, 4}]
=> SeriesData[x, 0, {1, 0, -1/2, 0, 1/24}, 0, 4, 1]

# Integrate: integrands with no rule stay unevaluated instead of overflowing the
# stack (found via the app's bug-report button), and bare symbols are covered.
# (Sin[x]/x now has a rule: see special.mt)
Integrate[Sin[x]/x^2, x]
=> Integrate[Sin[x]/x^2, x]
Integrate[x, x]
=> 1/2 x^2
Integrate[2 x, x]
=> x^2
Integrate[3 x^2 + 2 x + 1, x]
=> x + x^2 + x^3
