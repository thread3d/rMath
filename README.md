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

Everything is driven by the `Makefile` from the project root:

```bash
make help          # list all commands
make release       # build the kernel (release)
make notebook-bg   # start the web notebook in the background
make stop          # stop it
make restart       # stop + start again
```

The notebook then runs at **http://127.0.0.1:8787** — open it and evaluate
cells (Enter evaluates, Shift+Enter inserts a newline, ↑/↓ walk history,
Ctrl+Space autocompletes). Inputs and outputs are typeset with LaTeX, plots
render as SVG.

Without `make`:

```bash
cd rmath && ~/.cargo/bin/cargo build --release     # build the kernel
cd .. && python3 -m rmathpy.notebook --port 8787   # foreground server
```

### macOS app (native window)

```bash
make app            # build dist/rMath.app  (native window app, ~3.4 MB)
make app-install    # install into ~/Applications + register with Spotlight
open ~/Applications/rMath.app        # or double-click it in Finder
```

`rMath.app` is a native Cocoa application: it embeds a **WKWebView** in a real
window (no browser needed) and starts the bundled notebook server + Rust kernel
as child processes. The bundle is self-contained — kernel binary, notebook
server, KaTeX fonts/CSS/JS, and the demo files — needing only a `python3`.

- **Menus**: ⌘R reload, ⌘+ / ⌘- / ⌘0 zoom, standard Cut/Copy/Paste, ⌘Q quit.
- **Quitting stops everything**: the server and its kernel are terminated with
  the app (no orphan processes).
- **Startup**: it uses port 8787, automatically moving to the next free port if
  that one is taken, shows a "Starting rMath…" splash, then swaps in the
  notebook. If startup fails it shows the log path instead.
- **State**: working directory `~/Library/Application Support/rMath/work`
  (seeded with a copy of `demo/`), log `~/Library/Logs/rMath.log`.

Terminal controls shipped inside the bundle:

```bash
APP=~/Applications/rMath.app/Contents/MacOS
"$APP/rMath" --stop          # stop a running instance (native app)
"$APP/rMath" --no-window     # start server, wait for health, exit (self-test)
"$APP/rMath" --port=9000     # different port
"$APP/rMath-cli" --stop      # shell-launcher equivalent (browser-based)
"$APP/rMath-cli" --no-open   # start without opening a browser
```

Useful environment variables: `RMATH_PYTHON=/path/to/python3` (choosing the
interpreter), `RMATH_LOG_REQUESTS=1` (log every HTTP request to the app log),
and `RMATH_NO_NATIVE=1 make app` (build the older browser-launcher bundle
instead of the native app).

### Notebook interface

The window is a Mathematica-style notebook:

- **Ribbon/toolbar**: Evaluate, Evaluate All, New, Open, Save, Save Last Plot as
  SVG, Reset Kernel, zoom out/in, Help (all functional).
- **Menus** (native): rMath, File, Edit, Insert, Evaluation, Format, Cell,
  Graphics, Palettes, Window, Help — with the usual shortcuts, e.g. ⌘N new,
  ⌘O open, ⌘S save, ⇧⌘S screenshot, ⇧⏎ evaluate, ⇧⌘⏎ evaluate all,
  ⌥⌘R reset kernel, ⌘+ / ⌘- / ⌘0 zoom, ⌘? help. `rMath --dump-menus` prints
  the whole tree.
- **Cells**: every evaluation shows `In[n]:=` and `Out[n]=` labels with a cell
  bracket, inputs and outputs typeset through the kernel's TeX printer
  (StandardForm: square brackets for functions, `\frac` for rationals,
  `\sqrt`, Greek letters, rules as `→`). Plots render inline as SVG.
- **Keyboard**: **Shift+Enter** evaluates (Mathematica convention), **Enter**
  adds a new line to the cell, **Ctrl+Space** completes a function name,
  ↑/↓ walk history.
- **Insert/Palettes menus** insert `Pi`, `Infinity`, `Sqrt[`, `Integrate[`,
  `Sum[`, comparison and rule operators, plus ready-made Plot/Table examples.
- **Typesetting form toggle**: **StandardForm** (default: `Factor[x² - 1]`,
  upright names) ⇄ **TraditionalForm** (`Factor(x² - 1)`, italic function
  names, parentheses). Switch from the **Format** menu (⌘T / ⇧⌘T), ⌃⇧T, or the
  **Standard/Traditional** button in the ribbon; existing cells re-typeset
  instantly (both renderings come back from the kernel, so nothing is
  re-evaluated).
- **Palettes**: real floating palette windows (⌥⌘1…⌥⌘6 or the **Palettes**
  menu) — Basic Math, Algebra, Calculus, Linear Algebra, Plot, Patterns. They
  stay above the notebook and their buttons insert at the cursor; the math
  palette inserts true Unicode (`π ∞ √ ≤ ≥ ≠ → θ α β …`), which the kernel
  parses directly (√ is a prefix operator: `√16` → `4`, `45 °` → `π/4`).
- **Help** (⌘? or the Help button) opens a searchable list of every kernel
  function; click a name to insert it.

### Screenshots from the app

The app snapshots its own window — no Screen Recording permission is required,
because it renders its own view rather than capturing the display.

- **In the app**: **File ▸ Save Screenshot** (⌘S) writes
  `~/Desktop/rMath-<timestamp>.png` and reveals it in Finder.
- **From a terminal**, against a running instance:

  ```bash
  APP=~/Applications/rMath.app/Contents/MacOS/rMath
  "$APP" --screenshot /tmp/shot.png                      # viewport PNG
  "$APP" --screenshot /tmp/page.pdf                      # full page as PDF
  "$APP" --screenshot /tmp/shot.png \
         --js 'document.getElementById("inp").value="Plot[Sin[x],{x,0,6}]"; evaluate();' \
         --delay 6                                        # drive the page, then shoot
  ```

  Add `--palette "Basic Math"` to open a palette in the running app and
  capture *it* instead of the page. The app polls
  `<tmpdir>/rMath-shot.request` (line 1: destination, line 2: optional
  JavaScript, line 3: optional delay, line 4: `page` or `palette:<name>`) and
  answers through `rMath-shot.done`. `rMath --screenshot-palette <name>
  <out.png>` renders a palette standalone. `rMath --dump-menus` prints the
  menu tree.

### Verifying output without screen-capture permissions

macOS gates real screen capture (`screencapture`, AppleScript UI scripting)
behind TCC permissions. Rendering-based tools need none, and are just as
useful for checking the notebook:

```bash
make shot                       # render http://127.0.0.1:8787 through WebKit -> /tmp/rmath-shot.png
make shot JS='document.getElementById("inp").value="Plot[Sin[x],{x,0,2 Pi}]"; evaluate();'
make svg2png IN=plot.svg OUT=plot.png     # independent renderer (rsvg-convert)
qlmanage -t -s 800 -o /tmp page.html      # QuickLook preview of an HTML/SVG file
```

`make shot` uses `scripts/dev/screenshot.swift` — WebKit, the same engine the
app embeds — so it shows exactly what the window shows (minus window chrome).

If you *do* want real screen/window capture (to include the native window
frame), grant **Screen Recording** — and for UI scripting, **Accessibility** —
to the app that launched the harness (usually Terminal.app):
System Settings ▸ Privacy & Security ▸ Screen & System Audio Recording, then
restart that app. Note that restarting Terminal ends the current agent session.

### Command-line reference### Command-line reference

| Command | What it does |
|---------|--------------|
| `make build` / `make release` | build the kernel (debug / release) |
| `make test` | unit tests + golden end-to-end suite |
| `make repl` | interactive kernel REPL (`Quit[]` or Ctrl-D exits) |
| `make eval E='D[Sin[x]^2, x]'` | one-shot evaluation |
| `make demo` | evaluate `demo/demo.mt` headlessly |
| `make notebook` | notebook server in the foreground (Ctrl-C stops) |
| `make notebook-bg` | notebook server detached (log: `/tmp/rmath-notebook.log`) |
| `make stop` / `make restart` | stop / restart the server |
| `make app` / `make app-run` | build / build + launch the macOS app (native window) |
| `make app-install` / `make app-uninstall` | install into `~/Applications` / remove |
| `make app-stop` | stop the app's server |
| `make shot` | render the notebook/app page to PNG via WebKit (no permissions needed) |
| `make svg2png IN=… OUT=…` | convert an SVG plot to PNG with `rsvg-convert` |
| `make clean` | remove Rust build artifacts |

Port can be changed with `make notebook-bg PORT=9000`. Note that this Rust
toolchain was installed with `--no-modify-path`, so `cargo` lives at
`~/.cargo/bin/cargo`; add it to your shell with
`export PATH="$HOME/.cargo/bin:$PATH"` if you want to call it directly.

To check the server from a terminal:

```bash
curl -s http://127.0.0.1:8787/api/health
curl -s -X POST http://127.0.0.1:8787/api/eval -d '{"input":"1 + 2*3"}'
```

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