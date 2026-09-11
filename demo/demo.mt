(* rMath demonstration file.
   Load it from the notebook or REPL with:   Get["demo/demo.mt"]
   It prints a section-by-section tour of the system and ends with a plot. *)

Print["== 1. exact arithmetic =="]
1/2 + 1/3
2^100
N[Pi]

Print["== 2. functions, patterns and rules =="]
sq[x_] := x^2
sq[12]
fact[0] = 1; fact[n_] := fact[n - 1]*n
fact[10]
grade[s_] := "pass" /; s >= 60; grade[s_] := "fail" /; s < 60
grade[75]

Print["== 3. scoping =="]
Module[{x = 5}, x^2 + 1]
Block[{x = 100}, x/2]

Print["== 4. lists and functional programming =="]
data = Range[10]
Map[#^2 &, data]
Select[data, EvenQ]
Total[data]
Transpose[{{1, 2}, {3, 4}}]

Print["== 5. algebra =="]
Expand[(x + 1)^3]
Factor[x^3 - 6 x^2 + 11 x - 6]
Collect[x + x^2 + 2 x^2 + 3, x]

Print["== 6. calculus =="]
D[x^3 + Sin[x], x]
Integrate[1/x, x]
Integrate[x^2, {x, 0, 1}]
Normal[Series[Exp[x], {x, 0, 4}]]

Print["== 7. linear algebra =="]
m = {{2, 1}, {1, 3}}
Det[m]
Inverse[m]
LinearSolve[{{2, 1}, {1, 3}}, {5, 10}]

Print["== 8. equations =="]
Solve[x^2 + 5 x + 6 == 0, x]
NSolve[x^3 - 6 x^2 + 11 x - 6 == 0, x]

Print["== 9. strings =="]
"r" <> "Math"
StringLength["Mathematica-style"]

Print["== 10. file input / modify / output =="]
csv = Import["demo/data.csv"]
doubled = Map[2 # &, csv]
Export["demo/data_doubled.csv", doubled]
Import["demo/data_doubled.csv"]
FileExistsQ["demo/data_doubled.csv"]
Put["demo/saved.txt", x^2 + 2 x + 1]
Get["demo/saved.txt"]

Print["== 11. a plot =="]
Plot[Sin[x]^2 + Cos[x], {x, 0, 2 Pi}]
