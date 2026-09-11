# rMath — a stand-alone Mathematica-style system

A self-contained clone of the Mathematica *experience*: a symbolic kernel in
**Rust** (expression engine, pattern matching, exact arithmetic, CAS) plus a
Python front end (web notebook + SVG plots). No external crates, no pip
packages — builds from source with nothing but `cargo` and `python3`.

```
In[1]:= f[n_] := f[n-1]*n; f[0] = 1; f[10]
Out[1]= 3628800

In[2]:= D[Sin[x]^2, x]
Out[2]= 2 Cos[x] Sin[x]

In[3]:= Integrate[x^2, {x, 0, 1}]
Out[3]= 1/3

In[4]:= Plot[Sin[x], {x, 0, 6}]
Out[4]= <an SVG plot in the notebook>
```

## Quick start

```bash
# 1. Build the kernel (Rust)
cd rmath
cargo build --release

# 2. Start the notebook (Python, stdlib only)
cd ..
python3 -m rmathpy.notebook            # → http://127.0.0.1:8787
```

Then open `http://127.0.0.1:8787` in a browser and evaluate cells
(Enter or Shift+Enter submits; ↑/↓ walk history; Ctrl+Space autocompletes).

## Components

| Path          | What it is |
|---------------|------------|
| `rmath/`      | Rust kernel crate (`lib` + `rmath` binary) |
| `rmathpy/`    | Python front end: kernel client, SVG renderer, notebook server |
| `docs/PROTOCOL.md` | JSON-RPC stdio protocol between front end and kernel |
| `rmath/tests/golden/` | End-to-end golden tests (`expr` / `=> expected`) |

### Kernel CLI

```
rmath                  # interactive REPL (In[n]:= / Out[n]=)
rmath -e "1 + 2"       # one-shot evaluation
rmath --server         # JSON-RPC stdio mode (used by rmathpy)
rmath --version
```

### Python

```python
from rmathpy import Kernel

k = Kernel()
k.eval("1/2 + 1/3")["result"]        # "5/6"
k.eval("Plot[Sin[x], {x, 0, 6}]")["svg"]  # rendered SVG

k.reset()
k.eval("x = 5; x^2")["result"]       # "25"
```

## What the kernel supports

- **Numbers**: arbitrary-precision integers and exact rationals; machine
  reals; exact and machine complex numbers. Mixed arithmetic is exact until a
  real appears, exactly like Mathematica's `Rational`/`Real` semantics.
- **Syntax**: the Mathematica operator set — `+ - * / ^ . (implicit times)`,
  function application `f[x]`, prefix `f@x`, postfix `x // f`, infix `a~f~b`,
  `Map` `/@`, `Apply` `@@`/`@@@`, `Composition` `@*`, rules `-> :>`,
  assignments `=` `:=` `=.` `+=` `^=`, `Condition` `/;`, patterns
  `x_` `x__` `x___` `x_?test` `x_:default` `x_.` `x:pat`, `Alternatives`,
  `Part` `[[..]]` with `Span` (`;;`), slots `#`/`##`, `%` history,
  associations `<|k -> v|>`, strings, comments `(* *)`, base literals `2^^101`.
- **Evaluation**: standard evaluation with `HoldAll`-family attributes,
  `Sequence` splicing, `Unevaluated`, `Evaluate` in held args, ordered
  down-values (specific rules first), up-value-free v1, `Module`/`Block`/`With`
  scoping, `Catch`/`Throw`, `Break`/`Continue`/`Return`.
- **Algebra**: `Expand`, `ExpandAll`, `Together`, `Cancel` (v1 = Together),
  `Collect`, `Factor` (content + rational roots), `Coefficient(s)`,
  `CoefficientList`, `Exponent`, `Variables`, `PolynomialQuotient[Remainder]`,
  `PolynomialGCD` (univariate).
- **Calculus**: `D` (all elementary chain rules), `Integrate` (polynomials,
  powers, linear-arg trig/exp/log/sqrt, definite integrals),
  `Series` → `SeriesData`, `Normal`, `Limit` (series-based).
- **Linear algebra**: `Det`, `Inverse`, `Transpose`, `Dot`, `IdentityMatrix`,
  `DiagonalMatrix`, `MatrixPower`, `RowReduce`, `MatrixRank`, `LinearSolve`.
- **Solving**: `Solve` (linear systems, quadratic/cubic-with-rational-roots),
  `NSolve` (Durand-Kerner, all complex roots), `FindRoot`-style Newton (via
  `FixedPoint`).
- **Lists/functional**: `Table`, `Map`, `Apply`, `MapAll`, `MapThread`,
  `Thread`, `Select`, `Cases`, `Count`, `Position`, `Sort[By]`, `Reverse`,
  `Join`, `Append[To]`, `Take`, `Drop`, `Flatten`, `Union`,
  `Intersection`, `Complement`, `Total`, `Sum`, `Product`, `Range`, `Nest`,
  `NestList`, `Fold`, `FixedPoint`, `Sow`/`Reap`.
- **Numbers/predicates**: `PrimeQ`, `Prime`, `EvenQ`, `OddQ`, `NumberQ`,
  `NumericQ`, `AtomQ`, `Head`, `Length`, `MatchQ`, `FreeQ`, `MemberQ`,
  `SameQ`, `Equal`, ordering relations, `Abs`, `Sign`, `Floor`, `Ceiling`,
  `Round`, `Mod`, `Quotient`, `GCD`, `LCM`, `Re`, `Im`, `Conjugate`,
  `N`, `Chop`, elementary trig/exp/log/hyperbolics with exact special values.
- **Graphics**: `Plot`, `ListPlot`, `ListLinePlot`, `ParametricPlot` produce
  a `Graphics[...]` structure that the notebook renders as styled SVG
  (axes, ticks, labels, clipped curves, asymptote splitting).
- **Strings/system**: `StringJoin`, `StringLength`, `StringTake/Drop`,
  `Characters`, `ToString`, `ToExpression`, `Attributes`, `SetAttributes`,
  `Clear[All]`, `Definition`, `Names`, `SeedRandom`, `Random*`,
  `AbsoluteTiming`, `Print`, `Sow`/`Reap`.

## Known limitations (v1)

- Machine-precision `N[expr, prec]`: the precision argument is currently
  ignored (arbitrary-precision floats are planned via a decimal bigfloat).
- `Cancel` = `Together` (multivariate rational GCD is not implemented).
- `Solve` covers linear systems (unique solutions), quadratics, and cubics
  with rational roots; multivariate nonlinear systems stay unevaluated.
- `Eigenvalues` only for 2×2 matrices.
- Symbolic `Sum`/`Product` are not computed in closed form.
- Up-values (`f /: g[f[x]] = ...` parse but are not applied).

## Architecture

```
rmath/src/
  bigint.rs     arbitrary-precision integers (base 2^32, no deps)
  rational.rs   exact rationals
  json.rs       minimal JSON parser/writer for the RPC
  lexer.rs      Mathematica tokenizer
  parser.rs     recursive-descent parser (full operator precedence)
  expr.rs       expression model + symbol table (attributes, own/down-values)
  eval.rs       standard evaluation loop, attributes, scoping
  pattern.rs    pattern matcher (Orderless/Flat aware)
  builtins/     registry + arithmetic, algebra, calculus, linalg, graphics…
  fmt.rs        OutputForm / InputForm / FullForm printers
  rpc.rs        JSON-RPC stdio server
  repl.rs/main  REPL and CLI
```

The evaluator follows Mathematica's evaluation semantics: evaluate the head,
evaluate arguments according to hold attributes, splice `Sequence`, strip
`Unevaluated`, apply user rules, then built-ins, then iterate until a
fixpoint (bounded by `$IterationLimit`/`$RecursionLimit`).

## Testing

```
cd rmath
cargo test            # 32 unit tests + ~70 end-to-end golden cases
```

## License

MIT.