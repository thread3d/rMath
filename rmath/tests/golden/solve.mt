# Solve / FindRoot and complex arithmetic
# (the complex fixes were regression bugs: I times anything was 0)

# linear
Solve[2 x + 3 == 7, x]
=> {{x -> 2}}
# quadratic, rational roots
Solve[x^2 - 4 == 0, x]
=> {{x -> 2}, {x -> -2}}
# irrational roots
Solve[x^2 - 2 == 0, x]
=> {{x -> Sqrt[2]}, {x -> -Sqrt[2]}}
# complex roots
Solve[x^2 + 4 == 0, x]
=> {{x -> 2 I}, {x -> -2 I}}
# complex roots with radicals
Solve[x^2 + x + 1 == 0, x]
=> {{x -> 1/2 (-1 + I Sqrt[3])}, {x -> 1/2 (-1 - I Sqrt[3])}}
# simplified radical
Solve[x^2 - 8 == 0, x]
=> {{x -> 2 Sqrt[2]}, {x -> -2 Sqrt[2]}}
# symbolic linear
Solve[a x == b, x]
=> {{x -> b/a}}
# quartic, rational roots
Solve[x^4 - 5 x^2 + 4 == 0, x]
=> {{x -> 2}, {x -> -2}, {x -> 1}, {x -> -1}}
# biquadratic
Solve[x^4 + x^2 - 1 == 0, x]
=> {{x -> Sqrt[1/2 (-1 + Sqrt[5])]}, {x -> -Sqrt[1/2 (-1 + Sqrt[5])]}, {x -> Sqrt[1/2 (-1 - Sqrt[5])]}, {x -> -Sqrt[1/2 (-1 - Sqrt[5])]}}
# linear system
Solve[{x + y == 5, x - y == 1}, {x, y}]
=> {{x -> 3, y -> 2}}
# inconsistent
Solve[{x + y == 1, x + y == 2}, {x, y}]
=> {}
# numeric complex roots
NSolve[x^2 + 1 == 0, x]
=> {{x -> -1.0 I}, {x -> 1.0 I}}
# Newton
FindRoot[x^2 - 2 == 0, {x, 1}]
=> {x -> 1.41421}
# transcendental
FindRoot[Cos[x] == x, {x, 1}]
=> {x -> 0.739085}
# complex product
(1 + I)(1 - I)
=> 2
# complex product
(2 + 3 I)(1 - I)
=> 5 + I
# complex power
(1 + I)^2
=> 2 I
# I squared
I^2
=> -1
# imaginary multiple
3 I
=> 3 I
# negative radical
Sqrt[-4]
=> 2 I
# square factor
Sqrt[8]
=> 2 Sqrt[2]
