# Steps[expr] — labeled derivations — and Trace[expr] — the general
# evaluation trace. Each Step renders as `label: expr`.

Steps[D[Sin[x]^2, x]]
=> {Given: D[Sin[x]^2, x], chain rule: d/dx Sin: Cos[x], power rule: 2 Cos[x] Sin[x], Result: 2 Cos[x] Sin[x]}
Steps[D[x^3 + Sin[x], x]]
=> {Given: D[x^3 + Sin[x], x], power rule: 3 x^2, chain rule: d/dx Sin: Cos[x], sum rule: Cos[x] + 3 x^2, Result: Cos[x] + 3 x^2}

Steps[Integrate[x Sin[x], x]]
=> {Given: Integrate[x Sin[x], x], integration by parts: Sin[x] - x Cos[x], Result: Sin[x] - x Cos[x]}
Steps[Integrate[1/(x^2 - 1), x]]
=> {Given: Integrate[1/(x^2 - 1), x], partial fractions: -1/2/(1 + x) + 1/2/(-1 + x), Result: -1/2 Log[1 + x] + 1/2 Log[-1 + x]}

Steps[Limit[Sin[x]/x, x -> 0]]
=> {Given: Limit[Sin[x]/x, x -> 0], chain rule: d/dx Sin: Cos[x], power rule: 1, l'Hopital (0/0): 1, Result: 1}

Steps[Simplify[(x^2 - 1)/(x - 1)]]
=> {Given: Simplify[(x^2 - 1)/(x - 1)], cancel common factor: 1 + x, Result: 1 + x}
Steps[Simplify[Sqrt[x^2], x > 0]]
=> {Given: Simplify[Sqrt[x^2], Greater[x, 0]], assumption: Sqrt[x^2] = x: x, Result: x}
Steps[Simplify[Sin[x]^2 + Cos[x]^2]]
=> {Given: Simplify[Sin[x]^2 + Cos[x]^2], Pythagorean identity: 1, Result: 1}

Steps[Apart[1/(x^2 - 1), x]]
=> {Given: Apart[1/(x^2 - 1), x], partial fractions: -1/2/(1 + x) + 1/2/(-1 + x), Result: -1/2/(1 + x) + 1/2/(-1 + x)}
Steps[Factor[x^2 - 1]]
=> {Given: Factor[x^2 - 1], factorization: (-1 + x) (1 + x), Result: (-1 + x) (1 + x)}
Steps[Solve[x^2 - 3 x + 2 == 0, x]]
=> {Given: Solve[Equal[x^2 - 3 x + 2, 0], x], polynomial form: 2 + x^2 - 3 x, discriminant: b^2 - 4 a c: 1, quadratic formula: (-b +- Sqrt[disc])/(2 a): {2, 1}, roots: {{x -> 2}, {x -> 1}}, Result: {{x -> 2}, {x -> 1}}}
# Even-power quartic: substitute y = x^2, solve the quadratic in y, then x = ±Sqrt[y].
Steps[Solve[x^4 + x^2 - 1 == 0, x]]
=> {Given: Solve[Equal[x^4 + x^2 - 1, 0], x], polynomial form: -1 + x^2 + x^4, even powers: substitute y = x^2: -1 + y + y^2, discriminant: b^2 - 4 a c: 5, quadratic formula: (-b +- Sqrt[disc])/(2 a): {1/2 (-1 + Sqrt[5]), 1/2 (-1 - Sqrt[5])}, x = +- Sqrt[y]: {Sqrt[1/2 (-1 + Sqrt[5])], -Sqrt[1/2 (-1 + Sqrt[5])], Sqrt[1/2 (-1 - Sqrt[5])], -Sqrt[1/2 (-1 - Sqrt[5])]}, roots: {{x -> Sqrt[1/2 (-1 + Sqrt[5])]}, {x -> -Sqrt[1/2 (-1 + Sqrt[5])]}, {x -> Sqrt[1/2 (-1 - Sqrt[5])]}, {x -> -Sqrt[1/2 (-1 - Sqrt[5])]}}, Result: {{x -> Sqrt[1/2 (-1 + Sqrt[5])]}, {x -> -Sqrt[1/2 (-1 + Sqrt[5])]}, {x -> Sqrt[1/2 (-1 - Sqrt[5])]}, {x -> -Sqrt[1/2 (-1 - Sqrt[5])]}}}

# Nothing instrumented fires: the successive evaluation forms are shown.
Steps[1 + 2*3]
=> {Given: 1 + 2 3, eval: 6, eval: 7}
Trace[(1 + 2)*(3 + 4)]
=> {Given: (1 + 2) (3 + 4), eval: 3, eval: 7, eval: 21}
