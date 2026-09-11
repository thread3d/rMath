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
