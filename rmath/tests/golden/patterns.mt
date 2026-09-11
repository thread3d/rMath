# patterns, rules, functions
f[x_] := x^2; f[5]
=> 25
f[x_, y_] := x + y; f[1, 2]
=> 3
x /. x -> 5
=> 5
{a, b, c} /. {a -> 1, c -> 3}
=> {1, b, 3}
h[x_] := x /; x > 0; h[5]
=> 5
h[x_] := x /; x > 0; h[-1]
=> h[-1]
g[args__] := Length[{args}]; g[1, 2, 3]
=> 3
MatchQ[3, _Integer]
=> True
MatchQ[{1, 2}, _List]
=> True
Cases[{1, 2.5, 3}, _Integer]
=> {1, 3}
k[x_] := x^2; k /@ {1, 2, 3}
=> {1, 4, 9}
Replace[{1, 2, 3}, x_List -> Total[x]]
=> 6
fib[n_] := fib[n-1] + fib[n-2]; fib[0] = 0; fib[1] = 1; fib[10]
=> 55
Module[{x = 2}, x + 1]
=> 3
s = #^2 &; s[7]
=> 49
