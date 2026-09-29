# Symbolic closed forms for Sum and Product.

Sum[i, {i, 1, n}]
=> 1/2 n (1 + n)
Sum[i^2, {i, 1, n}]
=> 1/6 n (1 + n) (1 + 2 n)
Sum[i^3, {i, 1, n}]
=> 1/4 n^2 (1 + n)^2
Sum[3 i^2 + 2 i + 1, {i, 1, n}]
=> n + n (1 + n) + 1/2 n (1 + n) (1 + 2 n)
Sum[c, {i, 1, n}]
=> c n
Sum[i, {i, 3, 7}]
=> 25
Sum[i^2, {i, 2, 5}]
=> 54

# Infinite sums: Basel and geometric.
Sum[1/n^2, {n, 1, Infinity}]
=> 1/6 Pi^2
Sum[1/n^4, {n, 1, Infinity}]
=> 1/90 Pi^4
Sum[1/2^n, {n, 0, Infinity}]
=> 2
Sum[3/4^n, {n, 0, Infinity}]
=> 4
Sum[r^i, {i, 0, Infinity}]
=> 1/(1 - r)
Sum[x^i, {i, 0, n}]
=> (-1 + x^(1 + n))/(-1 + x)

# Unrecognised symbolic bounds stay unevaluated rather than aborting.
Sum[i^5, {i, 1, n}]
=> Sum[i^5, {i, 1, n}]
Sum[Sin[i], {i, 1, n}]
=> Sum[Sin[i], {i, 1, n}]
Sum[1/n^2, {n, 2, Infinity}]
=> Sum[1/n^2, {n, 2, Infinity}]

# Numeric iteration is unchanged.
Sum[i, {i, 1, 10}]
=> 55
Sum[1/i, {i, 1, 10}]
=> 7381/2520
Product[2, {i, 1, 5}]
=> 32

# Products.
Product[i, {i, 1, n}]
=> Factorial[n]
Product[c, {i, 1, n}]
=> c^n
