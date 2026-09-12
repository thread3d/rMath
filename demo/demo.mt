(* rMath demonstration file.

   Load it from the notebook or the native app with:

       Get["demo/demo.mt"]

   A file evaluated with Get does not echo intermediate values (as in
   Mathematica), so every step here prints its own result — the whole tour is
   visible. The final expression is returned, so the notebook renders the plot
   as a picture. *)

Print["== 1. exact arithmetic =="]
Print["  1/2 + 1/3        = ", 1/2 + 1/3]
Print["  1/3 + 1/6 + 1/2  = ", 1/3 + 1/6 + 1/2]
Print["  2^100            = ", 2^100]
Print["  20!              = ", Factorial[20]]
Print["  Sqrt[16]         = ", Sqrt[16]]
Print["  N[Pi]            = ", N[Pi]]

Print["== 2. functions, patterns and rules =="]
sq[x_] := x^2
Print["  sq[x_] := x^2 ; sq[12]        = ", sq[12]]
fact[0] = 1; fact[n_] := fact[n - 1]*n
Print["  fact[10] (recursive)          = ", fact[10]]
grade[s_] := "pass" /; s >= 60; grade[s_] := "fail" /; s < 60
Print["  grade[75] (conditional rules) = ", grade[75]]
Print["  Map[#^3 &, {1, 2, 3}]         = ", Map[#^3 &, {1, 2, 3}]]
Print["  {1, 2, 3, 4} /. x_Integer :> x^2 = ", {1, 2, 3, 4} /. x_Integer :> x^2]

Print["== 3. scoping =="]
Print["  Module[{x = 5}, x^2 + 1]  = ", Module[{x = 5}, x^2 + 1]]
Print["  Block[{x = 100}, x/2]     = ", Block[{x = 100}, x/2]]
Print["  With[{y = 3}, y + 1]      = ", With[{y = 3}, y + 1]]

Print["== 4. lists and functional programming =="]
Print["  Range[10]                 = ", Range[10]]
Print["  Map[#^2 &, Range[10]]     = ", Map[#^2 &, Range[10]]]
Print["  Select[Range[10], EvenQ]  = ", Select[Range[10], EvenQ]]
Print["  Total[Range[10]]          = ", Total[Range[10]]]
Print["  Fold[Plus, 0, Range[10]]  = ", Fold[Plus, 0, Range[10]]]
Print["  Transpose[{{1,2},{3,4}}]  = ", Transpose[{{1, 2}, {3, 4}}]]

Print["== 5. algebra =="]
Print["  Expand[(x + 1)^3]               = ", Expand[(x + 1)^3]]
Print["  Factor[x^3 - 6 x^2 + 11 x - 6]  = ", Factor[x^3 - 6 x^2 + 11 x - 6]]
Print["  Collect[x + x^2 + 2 x^2 + 3, x] = ", Collect[x + x^2 + 2 x^2 + 3, x]]
Print["  Together[1/x + 1/y]             = ", Together[1/x + 1/y]]

Print["== 6. calculus =="]
Print["  D[x^3 + Sin[x], x]                = ", D[x^3 + Sin[x], x]]
Print["  D[Exp[x^2], x]                    = ", D[Exp[x^2], x]]
Print["  Integrate[1/x, x]                 = ", Integrate[1/x, x]]
Print["  Integrate[x^2, {x, 0, 1}]         = ", Integrate[x^2, {x, 0, 1}]]
Print["  Normal[Series[Exp[x], {x, 0, 4}]] = ", Normal[Series[Exp[x], {x, 0, 4}]]]

Print["== 7. linear algebra =="]
m = {{2, 1}, {1, 3}}
Print["  m                        = ", m]
Print["  Det[m]                   = ", Det[m]]
Print["  Inverse[m]               = ", Inverse[m]]
Print["  LinearSolve[m, {5, 10}]  = ", LinearSolve[m, {5, 10}]]
Print["  MatrixPower[m, 2]        = ", MatrixPower[m, 2]]
Print["  RowReduce[m]             = ", RowReduce[m]]

Print["== 8. equations =="]
Print["  Solve[x^2 + 5 x + 6 == 0, x]           = ", Solve[x^2 + 5 x + 6 == 0, x]]
Print["  Solve[{x+y==5, x-y==1}, {x, y}]        = ", Solve[{x + y == 5, x - y == 1}, {x, y}]]
Print["  NSolve[x^3 - 6 x^2 + 11 x - 6 == 0, x] = ",
      NSolve[x^3 - 6 x^2 + 11 x - 6 == 0, x]]

Print["== 9. strings =="]
Print["  \"r\" <> \"Math\"                    = ", "r" <> "Math"]
Print["  StringLength[\"Mathematica-style\"] = ", StringLength["Mathematica-style"]]
Print["  ToUpperCase[\"rmath\"]              = ", ToUpperCase["rmath"]]

Print["== 10. file input / modify / output =="]
csv = Import["demo/data.csv"]
Print["  Import[\"demo/data.csv\"]  = ", csv]
doubled = Map[2 # &, csv]
Print["  doubled (in memory)       = ", doubled]
Export["demo/data_doubled.csv", doubled]
Print["  Export -> demo/data_doubled.csv ; re-import = ",
      Import["demo/data_doubled.csv"]]
Print["  FileExistsQ[\"demo/data_doubled.csv\"] = ", FileExistsQ["demo/data_doubled.csv"]]
Put["demo/saved.txt", x^2 + 2 x + 1]
Print["  Put + Get round-trip      = ", Get["demo/saved.txt"]]

Print["== 11. a plot (returned, so it renders as a picture) =="]
Print["  Plot[Sin[x]^2 + Cos[x], {x, 0, 2 Pi}]"]
Plot[Sin[x]^2 + Cos[x], {x, 0, 2 Pi}]
