# Language surface added in the hardening pass: Piecewise, string processing,
# association access, and common list primitives.

Piecewise[{{1, True}, {2, False}}]
=> 1
Piecewise[{{1, False}}, 7]
=> 7
# An undecided condition leaves the whole Piecewise unevaluated.
Piecewise[{{1, x > 0}}]
=> Piecewise[{{1, Greater[x, 0]}}]

StringSplit["a b  c"]
=> {"a", "b", "c"}
StringSplit["a,b,c", ","]
=> {"a", "b", "c"}
StringReplace["abcabc", "b" -> "X"]
=> "aXcaXc"
StringMatchQ["hello", "h*"]
=> True
StringMatchQ["hello", "h?llo"]
=> True
StringMatchQ["hello", "x*"]
=> False
StringContainsQ["hello", "ell"]
=> True
StringContainsQ["hello", "xyz"]
=> False

Keys[<|a -> 1, b -> 2|>]
=> {a, b}
Values[<|a -> 1, b -> 2|>]
=> {1, 2}
Lookup[<|a -> 1, b -> 2|>, b]
=> 2
Lookup[<|a -> 1|>, c]
=> Missing["KeyAbsent", c]
Lookup[<|a -> 1|>, c, 0]
=> 0
KeyExistsQ[<|a -> 1|>, a]
=> True
KeyExistsQ[<|a -> 1|>, z]
=> False

ConstantArray[0, 4]
=> {0, 0, 0, 0}
Array[#^2 &, 5]
=> {1, 4, 9, 16, 25}
Array[f, 3, 0]
=> {f[0], f[1], f[2]}
Partition[{1, 2, 3, 4, 5}, 2]
=> {{1, 2}, {3, 4}}
Accumulate[{1, 2, 3, 4}]
=> {1, 3, 6, 10}
Differences[{1, 4, 9, 16}]
=> {3, 5, 7}
Ordering[{3, 1, 2}]
=> {2, 3, 1}
TakeWhile[{1, 2, 3, 1}, # < 3 &]
=> {1, 2}
DeleteCases[{1, 2, 3, 4}, _?EvenQ]
=> {1, 3}
Riffle[{a, b, c}, 0]
=> {a, 0, b, 0, c}

# Machine reals in scientific notation must not reach KaTeX as `*^`.
TeXForm[N[Gamma[21]]]
=> "2.4329 \\times 10^{18}"

# Unicode division/fraction slashes paste in from rendered math.
Together[1∕x + 1∕y]
=> (x + y)/(x y)

# A chained comparison is one n-ary comparison, not (a > b) > c, which used to
# compare True with a number and stay unevaluated (regression: 3 > 2 > 1 gave
# Greater[True, 1]). A mixed chain is the conjunction of its pairs.
3 > 2 > 1
=> True
1 > 2 > 3
=> False
1 < 2 < 3
=> True
1 < 2 > 3
=> False
2 > 1 > 0 > -1
=> True
1 <= 1 <= 1
=> True
FullForm[Hold[1 > 2 > 3]]
=> "Hold[Greater[1, 2, 3]]"
a = 5; 1 > a > 3
=> False

# Equal and Unequal are n-ary: a chain of equalities, pairwise difference.
1 == 1 == 1
=> True
1 == 2 == 1
=> False
1 != 2 != 3
=> True
1 != 2 != 1
=> False
Equal[x, y]
=> Equal[x, y]
