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

### Demonstration file

`demo/demo.mt` is a tour of the kernel: **339 verified examples and 75 statements
in 25 sections**
(arithmetic, complex numbers, elementary and special functions, Unicode input,
patterns, rules, scoping, lists, functional programming, algebra, polynomials,
calculus, equations, linear algebra, strings, logic, control flow, information
utilities, random numbers, file I/O), then calculus cross-checked in Lean, how
to verify a cell, and the notebook's own features, plus a function inventory
taken from `Names[]`, ending with a plot.

Every example is re-executed against the kernel, **in context** (statement by
statement in one session, exactly as the notebook's Open does), by
`make demo-gen` — so an example that stops evaluating is reported rather than
left in the file as a lie. Load it with `Get["demo/demo.mt"]` (in the app:
**File ▸ Open…**, which loads each statement as its own cell).

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

### Reporting a bug

One action captures everything needed to diagnose a problem:

- **In the app**: the bug button in the ribbon, or **Help ▸ Report a Bug…**
  (⇧⌘B).
- **From a terminal**: `make report` (or `rMath --report`).

The button talks to the app through WebKit's script-message handler, so it works
whether the page is hosted by the app or (for everything except app-only
actions) in a browser; in a browser the button is dimmed and explains that
reports need the app.

It writes `bug-<timestamp>.md` + `bug-<timestamp>.png` into
`<workspace>/.shots/reports/` (the workspace path is recorded by
`make app-install`; otherwise `~/Desktop/rMath-reports`), and puts a
paste-ready line on the **clipboard**:

```
rMath bug report: /Users/threaded/projects/rMath/.shots/reports/bug-20260912-080147.md
screenshot: /Users/threaded/projects/rMath/.shots/reports/bug-20260912-080147.png
last error: 1/0 — Infinite expression encountered.
```

Paste that into the chat. The report contains the app/kernel/Python versions,
the port and log path, a JSON transcript of the last 12 cells (inputs, results,
messages, whether a plot was involved), the last error, the page's current
typesetting form, and the tail of the server log. The screenshot shows the
window as it was.

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

### Lean integration (Lean 4 + Mathlib)

rMath can hand a claim to **Lean 4** and let the kernel-checked proof decide it.
The project lives in `lean/` (Mathlib; its ~8 GB `.lake/` build tree is
gitignored):

| | |
|---|---|
| install | `elan` at `~/.elan`, toolchain `leanprover/lean4:v4.34.0-rc2` (pinned by `lean/lean-toolchain`, matching Mathlib), Mathlib via `lake exe cache get` |
| project | `RMATH_LEAN_PROJECT` overrides `lean/` |
| tests | `make lean-test` — translation and proof-shape tests, no Lean needed; `make lean-verify` — the real battery (proved *and* rejected claims), slow since Mathlib loads per claim |

**What gets verified.** `Lean.verify` translates the claim to a Lean statement
and runs `solve | norm_num | ring | ring_nf | positivity | linarith | nlinarith |
omega | simp | decide | rfl`, so only a tactic that *closes* the goal counts:

| claim | outcome |
|---|---|
| `1/2 + 1/3 = 5/6`, `2^10 = 1024`, `20! = 2432902008176640000` | verified |
| `Factor[x^2 - 1] = (-1 + x)(1 + x)`, `Expand[(x+1)^3] = 1 + 3x + 3x^2 + x^3` | verified (`ring`) |
| `Sqrt[2]^2 = 2`, `Sin[Pi/6] = 1/2` | verified (over ℝ) |
| `x^2 >= 0` | verified (`positivity`) |
| `Integrate[x^2, {x, 0, 1}] = 1/3` | verified (`integral_pow`) — kernel calculus cross-checked |
| `D[x^3 + Sin[x], x]`, `D[Exp[x^2], x]`, `D[Sin[x]^2, x]`, `D[3 x^4, x]`, `D[Cos[x], x]`, `D[x, x]`, `D[7, x]`, `D[x^2 + 2 x + 1, x]`, `D[1/x, x]`, `D[Sin[x]/x, x]`, `D[Log[x], x]`, `D[Sqrt[x], x]`, `D[Exp[x] Log[x], x]`, `D[Log[x^2 + 1], x]` | verified — the kernel's derivative cross-checked through explicit `HasDerivAt` wiring (**14 of 14** sampled shapes) |
| `1/2 + 1/3 = 2/3` (false) | **not proved** |

Two subtleties worth knowing, both found by testing rather than reasoning:

- **Every numeral carries its type.** Without ascription Lean defaults `1/2` to
  ℕ, where `1/2 + 1/3 = 2/3` is `0 = 0` — a *false positive*. Numerals are now
  ascribed (ℚ, or ℝ when `Real.*`/`Pi` appears) and `^`/`Nat.factorial`
  arguments as ℕ.
- **`solve`, not `first`.** `first | norm_num | ring` stops at `norm_num` when it
  merely simplifies the goal, leaving it unsolved; `solve` requires a tactic that
  closes it.

**Derivatives.** There is no `deriv` tactic in this Mathlib, so `D[...]` results
are checked by building a `HasDerivAt` proof term structurally
(`hasDerivAt_pow`/`_id`/`_const` for leaves, then `.add`, `.sub`, `.mul`,
`.const_mul`, `.neg`, `.pow`, `.inv`, `.div`, `.sin`, `.cos`, `.exp`, `.log`,
`.sqrt`), with the function form written exactly as each lemma states it because
`rw [h.deriv]` matches syntactically. Where a lemma needs a non-zero argument
(quotients, `Log`, `Sqrt`) the side condition is **emitted as a hypothesis** —
`example (x : ℝ) (h1 : x ≠ 0) : deriv (fun x => Real.sin x / x) x = …` — stated in
reduced form (`x ≠ 0`, not `id x ≠ 0`) because that is what `field_simp`
recognises. `Abs` is still refused: this Mathlib has no `HasDerivAt` lemma for it.

**Refused on purpose** (with the reason shown in the panel): `N[...]` machine
approximations, complex numbers, `GCD`/`LCM`, bare expressions that are not
claims, and derivative shapes needing side conditions.

**On speed: why verdicts are not server-backed yet.** `rmathpy/lean.py` contains
a full LSP client (`lean --server`: Mathlib loads once, 0.5–0.9 s per check
instead of ~10 s). It is **opt-in** (`RMATH_LEAN_SERVER=1`) and never decides
verdicts, because testing showed it publishes *empty* diagnostic batches for
files with genuine errors — `example : (1 : ℚ) = 2 := by norm_num` came back
with no diagnostics, which would have reported false claims as verified.
Verdicts therefore come from the authoritative one-shot `lake env lean` run (its
exit status and diagnostics are the elaborator's own), and repeated claims are
cached, so a re-check is instant.

Two further routes to a fast *and* trustworthy server were attempted and are
documented, not shipped: hand-driving `Elab.Frontend.processCommands` against an
`importModules` environment (compiles, but returns with an empty message log —
elaboration is deferred to the snapshot machinery), and `runFrontend` with
incremental snapshots (the save step needs `supportInterpreter`). Both are
recorded in `lean/RMathServer.lean`, which is marked experimental and is not
wired into any code path.

**In the app**: the Lean ribbon button (or **Help ▸ Lean… ⇧⌘L**) opens a floating panel
with a snippet box (Mathlib is imported automatically), a **Check** button, and
severity-coloured diagnostics with line/column; **Verify the last cell**
(⌥⌘V) takes the cell's input and result, shows the generated Lean statement and
the outcome. Endpoints: `GET /api/lean/status`, `POST /api/lean/check`,
`POST /api/lean/verify`.

### Harness integration (DeepSeek Harness)

The notebook can hand work to the [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)
running as an agent with **this project as its workspace root** — it can read the
repository, edit files and run commands, including the build and the tests.

- **Open the panel**: the robot button in the ribbon, or **Help ▸ Ask the
  Harness… (⇧⌘H)**. It floats: drag the title bar, resize from the corner,
  collapse with the chevron, and the geometry is remembered — so the notebook,
  the Lean panel and the harness can be read side by side.
- **Ask about a cell**: "Ask about the last cell" sends that cell's input, output
  and messages, and the agent explains — or fixes — what it finds.
- **Fix a reported bug**: after **Report a Bug**, the report cell offers
  *"Ask the harness to fix this"*; the agent reads the report, finds the bug,
  edits the code, rebuilds, and runs `make test` / `make uitest`, then summarises
  what changed.
- Reasoning and tool output stream into the panel (dimmed, so it reads as
  thinking rather than output); the final answer is rendered as **markdown** —
  headings, lists, tables-free GitHub-flavoured subset, fenced code blocks with
  their language label, blockquotes, links, and `$…$` math through KaTeX. The
  renderer is self-contained (no new dependency) and escapes the answer before
  applying any formatting, so model output cannot inject markup. Every run uses model calls, and its file edits land in the working
  tree (review them with `git diff`).

Requirements and knobs:

| | |
|---|---|
| `dsh` | found on `PATH`, at `~/.npm-packages/bin/dsh`, or via `DSH_BIN=…` |
| API key | headless runs need a provider key: paste it once with **Set API key…** (stored in `~/Library/Application Support/rMath/harness.env`, mode 0600, outside the repo) or export `DEEPSEEK_API_KEY`. Note the Harness *web* UI's browser-session credential is not a provider key, so it does not cover the one-shot profile. |
| thinking | a local Ollama streams the model's thinking as `reasoning`, while the harness adapter reads `reasoning_content`, so nothing appeared until the final answer. rMath starts a small stdlib proxy (`rmathpy/ollama_proxy.py`) that renames that one field, and routes harness runs through it with a per-run `--patch` overlay (the settings document outranks the environment, so the overlay also disables that row for the run). Set `RMATH_NO_THINKING_PROXY=1` to keep the direct endpoint. |
| endpoint | resolved in this order: `$DEEPSEEK_BASE_URL`, the keyfile's `DEEPSEEK_BASE_URL`, then **a local Ollama (`http://localhost:11434/v1`) if one answers**, else `https://api.deepseek.com`. For a local endpoint the key is a placeholder, since Ollama ignores it. The panel header shows the resolved endpoint and its source. |
| harness settings | `$DSH_HOME/settings.yaml` is shared by every profile: point the route at Ollama with `llm-deepseek: {baseURL: http://localhost:11434/v1}` and choose a model Ollama actually serves with `agent-default-model: {provider: deepseek-official, model: …}` (currently `ornith-1.5:35b`, a local reasoning model). The adapter POSTs to `<baseURL>/chat/completions`, so the `/v1` suffix is required. The panel header shows the endpoint and the model the harness will start on. |
| note on local models | a local model needs tool calling for agent runs; `ornith-1.5:35b` does (verified: it listed `demo/` from the file system). Expect minutes per run rather than seconds. |
| profile | `RMATH_DSH_PROFILE` (default `headless`) |
| workspace | `RMATH_HARNESS_ROOT` (default: this repository) |
| keyfile | `RMATH_HARNESS_KEYFILE` |

Endpoints: `GET /api/agent/status`, `POST /api/agent` (one task, JSON),
`POST /api/agent/stream` (NDJSON: `{"line": …}` events then `{"done": …, "answer": …}`),
`POST /api/agent/key`.

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
| `make report` | bug report (screenshot + transcript + log) from the running app |
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
- `Integrate` covers polynomials, `1/(a x + b)`, sums, constant factors, the
  elementary functions with linear arguments, and the special cases
  `Sin[c x]/(a x)`, `Cos[c x]/(a x)`, `Exp[c x]/(a x)`, `1/Log[x]`, `Exp[-x^2]`
  (integration by parts, partial fractions and trigonometric substitution are
  not implemented, so e.g. `Integrate[x Sin[x], x]` stays unevaluated).
- Special functions available: `Erf`, `Erfc`, `SinIntegral`, `CosIntegral`,
  `SinhIntegral`, `CoshIntegral`, `ExpIntegralEi`, `LogIntegral`, `Sinc`
  (machine-precision numeric evaluation, exact values at 0/±Infinity, and
  derivatives). Common abbreviations from other systems are accepted as
  aliases: `Si`, `Ci`, `Shi`, `Chi`, `Ei`, `li`, `erf`, `erfc` — note that
  Mathematica itself has no `Si`; it is an extension here.

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