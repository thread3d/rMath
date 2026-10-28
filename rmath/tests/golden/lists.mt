# lists, functional programming
Table[i^2, {i, 1, 5}]
=> {1, 4, 9, 16, 25}
Total[{1, 2, 3}]
=> 6
Sort[{3, 1, 2}]
=> {1, 2, 3}
Range[5]
=> {1, 2, 3, 4, 5}
Select[{1, 2, 3, 4}, EvenQ]
=> {2, 4}
Map[#^2 &, {1, 2, 3}]
=> {1, 4, 9}
Length[{a, b, c}]
=> 3
Reverse[{1, 2, 3}]
=> {3, 2, 1}
Sum[i, {i, 1, 10}]
=> 55
Join[{1, 2}, {3, 4}]
=> {1, 2, 3, 4}
First[{a, b}]
=> a
Rest[{a, b, c}]
=> {b, c}
Table[{i, j}, {i, 2}, {j, 2}]
=> {{{1, 1}, {1, 2}}, {{2, 1}, {2, 2}}}
If[2 > 1, yes, no]
=> yes
Which[1 > 2, a, 3 > 2, b]
=> b
Do[i, {i, 3}]
=> 

# list builtins must evaluate their list argument (regression: Range[10] was
# treated as its own argument list by Select/Cases/Count/SortBy)
Select[Range[10], EvenQ]
=> {2, 4, 6, 8, 10}
Select[Range[10], # > 5 &]
=> {6, 7, 8, 9, 10}
Select[Range[10], OddQ]
=> {1, 3, 5, 7, 9}
Cases[Range[10], _?EvenQ]
=> {2, 4, 6, 8, 10}
Count[Range[10], _?EvenQ]
=> 5
SortBy[Range[5], Identity]
=> {1, 2, 3, 4, 5}
Position[Range[5], 5]
=> {{5}}

# PrimeQ must never answer False for an integer the built-in test cannot
# handle: it stays unevaluated (regression: every integer beyond u64 came
# back False, so PrimeQ[2^89 - 1] denied a Mersenne prime).
PrimeQ[97]
=> True
PrimeQ[2^61 - 1]
=> True
PrimeQ[2^89 - 1]
=> PrimeQ[618970019642690137449562111]
PrimeQ[10^20]
=> PrimeQ[100000000000000000000]
PrimeQ[x]
=> PrimeQ[x]
PrimeQ[1/2]
=> False
PrimeQ[2.5]
=> False
