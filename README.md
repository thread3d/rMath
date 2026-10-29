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
make mcp           # MCP server on stdio (for an MCP client)
```

The notebook then runs at **http://127.0.0.1:8787** — open it and evaluate
cells (Enter evaluates, Shift+Enter inserts a newline, ↑/↓ walk history,
Ctrl+Space autocompletes). Inputs and outputs are typeset with LaTeX, plots
render as SVG. The same server is a self-describing JSON API (`GET /api`), and
`make mcp` exposes the kernel over the Model Context Protocol — see
[HTTP API](#http-api) and [MCP server](#mcp-server-model-context-protocol).

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

`demo/demo.mt` is a tour of the kernel: **389 verified examples and 79 statements
in 24 sections**
(arithmetic, complex numbers, elementary and special functions, Unicode input,
patterns, rules, scoping, lists, functional programming, algebra, polynomials,
calculus, equations, linear algebra, strings, logic, control flow, information
utilities, random numbers, file I/O), then calculus cross-checked in Lean, how
to verify a cell, and the notebook's own features, plus a function inventory
taken from `Names[]`, ending with a plot.

Every example is re-executed against the kernel, **in context** (statement by
statement in one session, exactly as the notebook's Open does), by
`make demo-gen` — so an example that stops evaluating is reported rather than
left in the file as a lie. Load it with `Get["demo/demo.mt"]`, which prints each
labelled step; in the app, **File ▸ Open…** loads each statement as its own
cell and turns a labelled example `Print["  <expr>  =  ", <expr>]` into the
bare `<expr>`, so every result gets an `Out[n]=` line.

### Notebook interface

The window is a Mathematica-style notebook:

- **Ribbon/toolbar**: Evaluate, Evaluate All, New, Open, Save, Save Last Plot as
  SVG, Reset Kernel, zoom out/in, Standard/Traditional form, Steps, Help (all
  functional).
- **Menus** (native): rMath, File, Edit, Insert, Evaluation, Format, Cell,
  Graphics, Palettes, Window, Help — with the usual shortcuts, e.g. ⌘N new,
  ⌘O open, ⌘S save, ⇧⌘S screenshot, ⇧⏎ evaluate, ⇧⌘⏎ evaluate all,
  ⇧⌘D show steps, ⌥⌘R reset kernel, ⌘+ / ⌘- / ⌘0 zoom, ⌘? help.
  `rMath --dump-menus` prints the whole tree.
- **Cells**: every evaluation shows `In[n]:=` and `Out[n]=` labels with a cell
  bracket, inputs and outputs typeset through the kernel's TeX printer
  (StandardForm: square brackets for functions, `\frac` for rationals,
  `\sqrt`, Greek letters, rules as `→`). Plots render inline as SVG.
- **Copying**: inputs and outputs are typeset, but copying a cell puts its
  *source* on the clipboard — the input as written, an output as its
  `InputForm` — so an expression can be pasted back without a fraction losing
  its `/` (`Together[1/x + 1/y]` used to copy as `Together[1x+1y]`). Unicode
  slashes and minus signs from rendered math (`∕ ⁄ −`) are accepted too.
- **Keyboard**: **Shift+Enter** evaluates (Mathematica convention), **Enter**
  adds a new line to the cell, **Ctrl+Space** completes a function name,
  ↑/↓ walk history.
- **Persistence**: cell inputs are autosaved (debounced) to
  `<work dir>/.rmath-session.json`, and replayed on load, so quitting and
  relaunching the app no longer loses the notebook. Replaying re-evaluates the
  cells, which also re-establishes any definitions they made. **New** clears the
  saved session.
- **Insert/Palettes menus** insert `Pi`, `Infinity`, `Sqrt[`, `Integrate[`,
  `Sum[`, comparison and rule operators, plus ready-made Plot/Table examples.
- **Typesetting form toggle**: **StandardForm** (default: `Factor[x² - 1]`,
  upright names) ⇄ **TraditionalForm** (`Factor(x² - 1)`, italic function
  names, parentheses). Switch from the **Format** menu (⌘T / ⇧⌘T), ⌃⇧T, or the
  **Standard/Traditional** button in the ribbon; existing cells re-typeset
  instantly (both renderings come back from the kernel, so nothing is
  re-evaluated).
- **Show steps**: the **Steps** toggle in the ribbon (or **Evaluation ▸ Show
  Steps**, ⇧⌘D) makes every evaluation also display a derivation under its
  result — the named operations from the given input to the answer (chain rule,
  integration by parts, l'Hôpital, partial fractions, Pythagorean identity, …),
  rendered in the current typesetting form. The kernel records the steps during
  the same evaluation, so nothing is evaluated twice; the setting is remembered
  between sessions.
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
| install without a writable `$HOME` | elan is found at `$ELAN_BIN`, then `$ELAN_HOME/bin`, then `<repo>/.elan/bin`, then `~/.elan/bin` — on a machine whose home directory is read-only, `ELAN_HOME=$PWD/.elan` and `MATHLIB_CACHE_DIR=$PWD/.mathlib-cache` keep the ~3 GB toolchain and the ~450 MB download cache inside the repository (both gitignored) |
| project | `RMATH_LEAN_PROJECT` overrides `lean/` |
| toolchain | a system `lean`/`lake` of another version cannot build Mathlib at the revision `lean/lake-manifest.json` pins, so the bridge and `make lean-verify` prefer elan's shims (`lake` then runs the version `lean-toolchain` names) |
| availability | `rmathpy.lean.available()` (and `GET /api/lean/status`, which also reports `elan` and `mathlib`) requires **`lake` and a built `Mathlib.olean`** — `lake` alone parses `import Mathlib` as an unknown module, so every check would fail |
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
| `Integrate[x^2, x] = 1/3 x^3`, `Integrate[Sin[x], x] = -Cos[x]`, `Integrate[1/x, x] = Log[x]` | verified — an indefinite integral is checked by differentiating the antiderivative (`deriv F = f`) |
| `D[x^3 + Sin[x], x]`, `D[Exp[x^2], x]`, `D[Sin[x]^2, x]`, `D[3 x^4, x]`, `D[Cos[x], x]`, `D[x, x]`, `D[7, x]`, `D[x^2 + 2 x + 1, x]`, `D[1/x, x]`, `D[Sin[x]/x, x]`, `D[Log[x], x]`, `D[Sqrt[x], x]`, `D[Exp[x] Log[x], x]`, `D[Log[x^2 + 1], x]` | verified — the kernel's derivative cross-checked through explicit `HasDerivAt` wiring (**14 of 14** sampled shapes) |
| `Solve[x^2 - 3 x + 2 == 0, x]`, `Solve[x^2 - 2 == 0, x]` | verified — every root the kernel returned is substituted back into the equation |
| `Det[{{2, 1}, {1, 3}}] = 5`, `Transpose[...]`, `Dot[...]` (matrix·vector, vector·matrix, vector·vector) | verified — a matrix literal is `!![...]` over ℚ and the goal is closed entrywise (`ext i j <;> fin_cases i <;> fin_cases j <;> simp`, `norm_num [Matrix.det_fin_two]`) |
| `1/2 + 1/3 = 2/3` (false) | **not proved** |

`Steps[...]` and `Trace[...]` are display wrappers, so verifying such a cell
checks the expression they derive against that expression's own value (the Step
list itself has no Lean translation). A `Steps[D[Sin[x]^2, x]]` cell therefore
verifies as the derivative; an inner expression the bridge cannot translate
(e.g. `Solve`) is reported by its own name.

`Solve[...]` is checked the same way: the kernel finds the roots, and Lean is
asked the independent question whether each really satisfies the equation.
Non-real roots are excluded — Lean's `Real.sqrt` of a negative is 0, so a claim
about them would be false — and an all-complex result is refused; a root needing
deeper radical reasoning can come back **not proved** rather than verified.

**Automation and trust (Lean backlog).** Generated files inline
`lean/RMathLemmas.lean`, which defines `rmath_radical` (a best-effort nested
square-root closer) plus a few `@[simp]` facts; the closing tactic list also
tries `field_simp`, `norm_cast`, `gcongr`, `simp_all`, `aesop` and `grind`. Every
claim is a named `theorem rmath_check` followed by `#print axioms rmath_check`,
so the panel shows which axioms closed it and a proof that used `sorry` is never
reported as verified. All-complex `Solve` results are now checked over **ℂ**
instead of refused. A numeric fuzz cross-check (random values through the kernel)
downgrades a verdict to *cannot verify* if the two sides disagree, catching
translation bugs. **Persistence (latency).** `Lean.verify` takes its verdict from
the persistent LSP server once that server has *calibrated* — it must report a
known-false claim as an error — reusing one session file so Mathlib loads once:
**~2–3 s per claim instead of ~10 s**. A check is only conclusive when the
server signals elaboration completion (`$/lean/fileProgress`); anything
inconclusive falls back to the one-shot `lake env lean` run, so a missing
diagnostics batch can never become a verdict. The panel's **Fast check** exposes
the server's diagnostics directly, and **Find lemmas** searches Loogle. Set
`RMATH_LEAN_SERVER=0` to force the one-shot path. `make lean-bench` scores
translation and verification over the golden files and the demo (translation
coverage is **127 of 734** corpus claims: indefinite integrals, finite sums and
the `Collect`-like heads are translated now);
`RMATH_LEAN_HAMMER=1` retries *not proved* goals with external hammers (`duper`,
`lean_smt`) when the project supplies them.

`rmath_radical` walks the radicals in the goal bottom-up: for each radicand
`t` (innermost first) it proves `0 ≤ t`, records `(√t)² = t`, then closes the
goal with `nlinarith`. That is what `√((√5−1)/2)` needs — `0 ≤ (√5−1)/2`
follows from `(√5)² = 5` and `√5 ≥ 0` — so
`Solve[x^4 + x^2 - 1 == 0, x]` now verifies over ℝ (its two real roots; the
two non-real ones are excluded, since Lean's `Real.sqrt` of a negative is 0).
A radical whose sign is genuinely unprovable makes the tactic fail rather than
invent a verdict.

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

**Setting it up (once per checkout).** The toolchain, Mathlib and its cache are
per-checkout state, so they are not in git; two commands manage them:

```bash
make lean-status    # what is present, and what is missing (no Lean needed)
make lean-setup     # install whatever is missing — idempotent
```

`make lean-setup` installs `elan` into `.elan/` (unless `ELAN_HOME` says
otherwise) with the toolchain `lean/lean-toolchain` pins, downloads the Mathlib
build cache into `.mathlib-cache/`, runs `lake exe cache get` and `lake build`,
then reports through `make lean-status`. All three directories are gitignored
(~11 GB together), so a fresh clone or a `git clean -xdf` starts empty and
`make lean-setup` puts it back. `make lean-verify` refuses to run in that state
instead of skipping silently — a green suite that never checked a proof is the
failure mode this exists to prevent.

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
| tool lifecycle | harness runs go through the harness's **ACP** profile (`rmathpy/acp.py`, newline-delimited JSON-RPC), which publishes the agent's thoughts, its messages, and the generic tool lifecycle: `🔧 glob {"pattern":"demo/*"} [in_progress]` then `✓ glob [completed] <output>`, plus context usage. One session is reused, so follow-ups keep context. `RMATH_NO_ACP=1` falls back to the headless profile (reasoning + final answer only). Note: the ACP client answers the harness's permission requests itself — tools the agent asks for are allowed without a prompt, so treat a harness run as trusted automation. |
| thinking | a local Ollama streams the model's thinking as `reasoning`, while the harness adapter reads `reasoning_content`, so nothing appeared until the final answer. rMath starts a small stdlib proxy (`rmathpy/ollama_proxy.py`) that renames that one field, and routes harness runs through it with a per-run `--patch` overlay (the settings document outranks the environment, so the overlay also disables that row for the run). Set `RMATH_NO_THINKING_PROXY=1` to keep the direct endpoint. |
| endpoint | resolved in this order: `$DEEPSEEK_BASE_URL`, the keyfile's `DEEPSEEK_BASE_URL`, then **a local Ollama (`http://localhost:11434/v1`) if one answers**, else `https://api.deepseek.com`. For a local endpoint the key is a placeholder, since Ollama ignores it. The panel header shows the resolved endpoint and its source. |
| harness settings | `$DSH_HOME/settings.yaml` is shared by every profile: point the route at Ollama with `llm-deepseek: {baseURL: http://localhost:11434/v1}` and choose a model Ollama actually serves with `agent-default-model: {provider: deepseek-official, model: …}` (currently `ornith-1.5:35b`, a local reasoning model). The adapter POSTs to `<baseURL>/chat/completions`, so the `/v1` suffix is required. The panel header shows the endpoint and the model the harness will start on. |
| model | the panel's **Model** menu lists the models the resolved endpoint advertises (and the local Ollama's) and remembers your choice in `~/Library/Application Support/rMath/model`. It is applied with a per-run `--patch` overlay, so your `settings.yaml` is never modified; choosing "(default model)" clears it and falls back to `agent-default-model`. |
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

### Command-line reference

| Command | What it does |
|---------|--------------|
| `make build` / `make release` | build the kernel (debug / release) |
| `make test` | unit tests + golden end-to-end suite |
| `make py-test` | Python front-end unit tests (notebook + API, MCP, SVG, paths, harness, Lean translation) |
| `make check` | the fast CI set: `test` + `py-test` + `oracle` + `lean-test` |
| `make oracle` | random + invariant correctness oracle over the algebra surface (`ORACLE="-n 200 --seed 7"` for a longer run) — independent of the kernel's own simplifier |
| `make lean-status` | report what Lean needs and what is missing (no Lean required) |
| `make lean-setup` | install elan + the pinned toolchain + Mathlib into this checkout (idempotent) |
| `make lean-verify` | the Lean battery, proved *and* rejected claims (slow; needs `lean-setup`) |
| `make repl` | interactive kernel REPL (`Quit[]` or Ctrl-D exits) |
| `make eval E='D[Sin[x]^2, x]'` | one-shot evaluation |
| `make demo` | evaluate `demo/demo.mt` headlessly |
| `make notebook` | notebook server in the foreground (Ctrl-C stops) |
| `make notebook-bg` | notebook server detached (log: `/tmp/rmath-notebook.log`) |
| `make stop` / `make restart` | stop / restart the server |
| `make mcp` | MCP server on stdio (an MCP client launches it) |
| `make mcp-http` | MCP server over Streamable HTTP (`MCP_PORT=…`, default 8790) |
| `make app` / `make app-run` | build / build + launch the macOS app (native window) |
| `make app-install` / `make app-uninstall` | install into `~/Applications` / remove |
| `make app-stop` | stop the app's server |
| `make report` | bug report (screenshot + transcript + log) from the running app |
| `make shot` | render the notebook/app page to PNG via WebKit (no permissions needed) |
| `make svg2png IN=… OUT=…` | convert an SVG plot to PNG with `rsvg-convert` |
| `make clean` | remove Rust build artifacts |

`make oracle` is the correctness net: it feeds random expressions to the kernel
and checks each operation's invariants numerically (`Expand[Factor[p]] = p`,
every `Solve` root satisfies its equation, `D[Integrate[f]] = f`, and `Cancel` /
`Apart` must be finite at a removed root). Because it does not use the kernel's
own simplifier, it catches plausible-but-wrong answers and evaluation loops. It
found three real bugs: `canon_cmp` compared `Int`/`Rat` in the same direction
for both argument orders, so the Orderless sort was not antisymmetric and some
`Integrate` calls looped until `$IterationLimit`; `Cancel` was a straight alias
for `Together` and cancelled nothing; and `Apart` of a polynomial returned an
unevaluated `Apart` head.

Port can be changed with `make notebook-bg PORT=9000`. Note that this Rust
toolchain was installed with `--no-modify-path`, so `cargo` lives at
`~/.cargo/bin/cargo`; add it to your shell with
`export PATH="$HOME/.cargo/bin:$PATH"` if you want to call it directly.

To check the server from a terminal:

```bash
curl -s http://127.0.0.1:8787/api/health
curl -s -X POST http://127.0.0.1:8787/api/eval -d '{"input":"1 + 2*3"}'
```

### HTTP API

The notebook server is also the app's JSON API, and it describes itself:

```bash
curl -s http://127.0.0.1:8787/api                  # index: every route + the MCP entry point
curl -s http://127.0.0.1:8787/api/openapi.json     # OpenAPI 3.1
curl -s http://127.0.0.1:8787/api/names
```

The route table lives in `rmathpy/apispec.py`; a unit test keeps the discovery
index and the OpenAPI document in step with each other. The groups:

| Group | Routes |
|-------|--------|
| kernel | `GET /api/names`; `POST /api/eval`, `/api/reset`, `/api/split` |
| files | `POST /api/save`, `/api/open`, `/api/save-svg`, `/api/session`, `/api/autosave` |
| Lean | `GET /api/lean/status`; `POST /api/lean/check`, `/api/lean/verify`, `/api/lean/search`, `/api/lean/explore` |
| harness | `GET /api/agent/status`; `POST /api/agent`, `/api/agent/stream` (NDJSON), `/api/agent/cancel`, `/api/agent/key`, `/api/agent/model` |

Bodies and responses are JSON, except `POST /api/agent/stream`, which streams
newline-delimited JSON events. The server binds to `127.0.0.1` and refuses a
foreign `Host`/`Origin` (DNS-rebinding and cross-site protection), so a `curl`
with no browser headers is also a health check.

### MCP server (Model Context Protocol)

`rmathpy/mcp.py` exposes the kernel over MCP, so an agent can do exact
mathematics as tool calls. Standard library only, matching the rest of the
front end:

```bash
make mcp                    # stdio — what an MCP client launches
make mcp-http               # Streamable HTTP on 127.0.0.1:8790 (MCP_PORT=… to change)
python3 -m rmathpy mcp      # the same as `make mcp`
python3 -m rmathpy mcp --http --port 8790
```

A client configuration is just a command entry (the `cwd` matters when the
kernel binary is not on `PATH`):

```json
{
  "mcpServers": {
    "rmath": {
      "command": "python3",
      "args": ["-m", "rmathpy.mcp"],
      "cwd": "/path/to/rMath"
    }
  }
}
```

| Tool | What it does |
|------|--------------|
| `rmath_eval` | evaluate rMath statements in one persistent session; returns the In/Out transcript, and an SVG block when the result is graphics |
| `rmath_plot` | render a graphics expression to SVG (`width`/`height` optional) |
| `rmath_split` | split a script into top-level statements |
| `rmath_names` | list the symbols the kernel knows about |
| `rmath_reset` | clear every user definition |
| `rmath_help` | a short syntax cheat sheet, for the model to consult |
| `rmath_lean_check` / `rmath_lean_verify` | type-check a Lean snippet / prove an rMath claim in Mathlib — advertised only when `lake` is found |

The server implements the MCP lifecycle (`initialize` version negotiation,
`notifications/initialized`, `tools/list`, `tools/call`, `ping`, `resources/list`)
at protocol revision `2025-11-25`, echoing the client's revision when it is
supported (down to `2024-11-05`). stdio uses newline-delimited JSON-RPC and
writes only MCP messages to stdout; Streamable HTTP answers `POST /mcp` in JSON
response mode and applies the same origin check as the notebook.

## Components

| Path          | What it is |
|---------------|------------|
| `rmath/`      | Rust kernel crate (`lib` + `rmath` binary) |
| `rmathpy/`    | Python front end: kernel client, SVG renderer, notebook server, MCP server |
| `rmathpy/mcp.py` | MCP server (stdio + Streamable HTTP) over the kernel |
| `rmathpy/apispec.py` | HTTP API discovery index + OpenAPI 3.1 document |
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
from rmathpy import Kernel, eval_svg

k = Kernel()
k.eval("1/2 + 1/3")["result"]        # "5/6"

r, svg = eval_svg(k, "Plot[Sin[x], {x, 0, 6}]")   # svg is the rendered plot

k.reset()
k.eval("x = 5; x^2")["result"]       # "25"
```

## What the kernel supports

- **Numbers**: arbitrary-precision integers and exact rationals; machine
  reals; exact and machine complex numbers; and **arbitrary-precision decimal
  reals** from `N[expr, prec]`. Mixed arithmetic is exact until a real appears,
  exactly like Mathematica's `Rational`/`Real` semantics.
- **Syntax**: the Mathematica operator set — `+ - * / ^ . (implicit times)`,
  function application `f[x]`, prefix `f@x`, postfix `x // f`, infix `a~f~b`,
  `Map` `/@`, `Apply` `@@`/`@@@`, `Composition` `@*`, rules `-> :>`,
  assignments `=` `:=` `=.` `+=` `^=`, `Condition` `/;`, patterns
  `x_` `x__` `x___` `x_?test` `x_:default` `x_.` `x:pat`, `Alternatives`,
  `Part` `[[..]]` with `Span` (`;;`), slots `#`/`##`, `%` history,
  associations `<|k -> v|>`, strings, comments `(* *)`, base literals `2^^101`.
- **Evaluation**: standard evaluation with `HoldAll`-family attributes,
  `Sequence` splicing, `Unevaluated`, `Evaluate` in held args, ordered
  down-values (specific rules first), up-values tagged with `/:` or `^=`, `Module`/`Block`/`With`
  scoping, `Catch`/`Throw`, `Break`/`Continue`/`Return`. Error control:
  `Quiet`, `Check`, `Off`/`On`, `Abort`, `TimeConstrained`.
- **Derivations**: `Steps[expr]` returns the derivation of a result as a list of
  `Step[label, expr]` — named operations from the given input to the answer.
  It is instrumented for `D`, `Integrate`, `Limit`, `Simplify`, `Apart`,
  `Factor` and `Solve` (e.g. "chain rule: d/dx Sin", "integration by parts",
  "l'Hôpital (0/0)", "partial fractions", "Pythagorean identity"; `Solve` shows
  the polynomial form, an even-power substitution `y = x^2`, the discriminant,
  the quadratic formula and rational roots), and falls back to `Trace[expr]`,
  the successive evaluation forms, for anything else.
- **Algebra**: `Expand`, `ExpandAll`, `Together`, `Cancel` (cancels common
  factors, univariate),
  `Apart` (partial fractions, distinct linear factors), `Collect`, `Factor`
  (content + rational roots), `Coefficient(s)` (symbolic coefficients and
  monomial forms, on unexpanded input), `CoefficientList`, `Exponent`,
  `Variables`, `PolynomialQuotient[Remainder]`, `PolynomialGCD` (univariate),
  and `Simplify`/`FullSimplify` with `Assuming`/`$Assumptions`.  `Simplify` also applies the double-angle identity
  (`Sin[u] Cos[u]` → `Sin[2u]/2`).
- **Calculus**: `D` (elementary chain rules, and `Derivative` for unknown
  functions), `Integrate` (polynomials, powers incl. `Sqrt`, `1/(a x + b)`,
  rational functions via partial fractions, `x^n Sin/Cos/Exp` by parts,
  `x^n Log[x]`, `1/(c0 + c2 x^2)` → `ArcTan`, `1/Sqrt[c0 - c2 x^2]` → `ArcSin`,
  the special-function cases, and definite integrals),
  `Series` → `SeriesData`, `Normal`, `Limit` (l'Hôpital for 0/0 forms, with a
  series fallback; unresolved limits stay unevaluated).
- **Linear algebra**: `Det`, `Inverse`, `Transpose`, `Dot`, `IdentityMatrix`,
  `DiagonalMatrix`, `MatrixPower`, `RowReduce`, `MatrixRank`, `LinearSolve`,
  `Eigenvalues` and `Eigenvectors` for 2×2 matrices with exact entries (the
  latter return one unnormalised eigenvector per eigenvalue).
- **Solving**: `Solve` (linear systems, quadratic/cubic-with-rational-roots),
  `NSolve` (Durand-Kerner, all complex roots), `FindRoot`-style Newton (via
  `FixedPoint`).
- **Lists/functional**: `Table`, `Map`, `Apply`, `MapAll`, `MapThread`,
  `Thread`, `Select`, `Cases`, `Count`, `Position`, `Sort[By]`, `Reverse`,
  `Join`, `Append[To]`, `Take`, `Drop`, `Flatten`, `Union`,
  `Intersection`, `Complement`, `Total`, `Sum`, `Product` (symbolic closed
  forms for polynomials, geometric series, factorials and the Basel sums),
  `Range`, `Nest`, `NestList`, `Fold`, `FixedPoint`, `Sow`/`Reap`, plus
  `Array`, `ConstantArray`, `Partition`, `Accumulate`, `Differences`,
  `Ordering`, `TakeWhile`, `DeleteCases`, `Riffle`.
- **Numbers/predicates**: `Max`, `Min`, `Binomial`, `PrimeQ`, `Prime`, `EvenQ`, `OddQ`, `NumberQ`,
  `NumericQ`, `AtomQ`, `Head`, `Length`, `MatchQ`, `FreeQ`, `MemberQ`,
  `SameQ`, `Equal`, ordering relations, `Abs`, `Sign`, `Floor`, `Ceiling`,
  `Round`, `Mod`, `Quotient`, `GCD`, `LCM`, `Re`, `Im`, `Conjugate`,
  `N`, `Chop`, elementary trig/exp/log/hyperbolics with exact special values.
- **Graphics**: `Plot`, `ListPlot`, `ListLinePlot`, `ParametricPlot` produce
  a `Graphics[...]` structure that the notebook renders as styled SVG
  (axes, ticks, labels, clipped curves, asymptote splitting).
- **Strings/system**: `StringJoin`, `StringLength`, `StringTake/Drop`,
  `StringSplit`, `StringReplace`, `StringMatchQ`, `StringContainsQ`,
  `Characters`, `ToString`, `ToExpression`, association access (`Keys`,
  `Values`, `Lookup`, `KeyExistsQ`), `Piecewise`, `Attributes`,
  `SetAttributes`, `Clear[All]`, `Definition`, `Names`, `SeedRandom`,
  `Random*`, `AbsoluteTiming`, `Print`, `Sow`/`Reap`.

## Known limitations (v1)

- `N[expr, prec]` above machine precision is evaluated with the
  arbitrary-precision decimal for exact numbers, `Pi`/`E`/`Degree`, `+ - * /`,
  integer and rational powers, `Sqrt`, the elementary functions and anything
  that stays symbolic — `N[Pi, 50]`, `N[1/3, 40]`, `N[Sqrt[2], 50]`,
  `N[Sin[1], 40]`, `N[Log[2], 40]`, `N[ArcTan[1], 40]`, `N[Pi^Pi, 30]` all
  give `prec` real digits, and it threads over lists. A symbolic argument
  (`N[Sin[x], 40]`) and an `f64` input (`N[1.5, 40]`, which cannot regain
  precision) still report `N::meprec` and fall back to machine precision.
- `Apart` splits a denominator whose factors are linear (with multiplicity,
  including repeated roots) plus an irreducible quadratic, to the second power; a
  cube or higher power of a quadratic makes `Apart` leave the expression alone
  (`Integrate` does handle those — see below — it just works from the whole
  fraction rather than from a partial-fraction split).
- `Cancel` cancels the common monomial factor and the numeric content of any
  rational function, the common univariate polynomial factor, and a shared
  multivariate factor such as `x + y` (`Cancel[(x^2 - y^2)/(x - y)]` → `x + y`,
  and `Cancel[(a x + a)/(x + 1)]` → `a` — a symbolic coefficient is just another
  variable). The multivariate GCD uses a Kronecker substitution into one
  variable, and keeps a candidate only when exact multivariate division confirms
  it divides both sides, so a degenerate substitution loses a factor rather than
  inventing one.
- `Solve` covers linear systems (unique solutions), quadratics, and cubics
  with rational roots; multivariate nonlinear systems stay unevaluated.
- A `MatrixPower[...]` claim translates to Lean but is not proved by the tactic
  chain (`m ^ 3` does not reduce for `decide`); `Inverse`, `LinearSolve`,
  `RowReduce` and `MatrixRank` are not translated at all.
- `Eigenvalues`/`Eigenvectors` cover 2×2 matrices with exact entries (the
  characteristic quadratic is solved exactly, with `I` for a negative
  discriminant); larger matrices stay unevaluated.
- Symbolic `Sum`/`Product` cover constants, polynomials in the index up to
  degree 4, geometric series (finite and infinite), factorials and the Basel
  sums `Sum[1/n^2]`/`Sum[1/n^4]`; general symbolic summation and `NSum` are
  not implemented. Anything unrecognised stays unevaluated.
- `PrimeQ` is a deterministic Miller-Rabin test over machine-sized integers;
  a larger argument stays unevaluated with `PrimeQ::bigint` instead of being
  answered (a wrong `False` for a large prime would be worse than no answer).
  `Prime[n]` is tabulated up to `n = 200000`.
- Exact trigonometric values are tabulated for multiples of `Pi`, `Pi/2`,
  `Pi/3`, `Pi/4` and `Pi/6` (with `ComplexInfinity` where the denominator
  vanishes); any other rational multiple, such as `Sin[Pi/5]`, stays
  unevaluated rather than dropping to a machine approximation.
- Up-values are applied: `f /: g[f[x_]] := ...` (and `lhs ^= rhs` /
  `lhs ^:= rhs`, which take the tag from the head of `lhs`) fires when an
  expression containing the tag is evaluated, including under a built-in head.
- `Integrate` covers polynomials, `1/(a x + b)`, sums, constant factors,
  rational functions that split into distinct linear factors (via `Apart`),
  `x^n Sin/Cos/Exp` and `x^n Log[x]` by parts, the arc-tangent/arc-sine forms
  of `1/quadratic`, any power `Sin[u]^n`/`Cos[u]^n` of a linear argument (by the
  reduction formula, so `Integrate[Sin[x]^2, x]` and `Integrate[Cos[x]^4, x]`
  work), the elementary functions with linear arguments, rational functions
  with a single irreducible quadratic factor (`Integrate[1/(x^3 + x), x]` →
  `Log[x] - 1/2 Log[1 + x^2]`, via `Apart`), and the special cases
  `Sin[c x]/(a x)`, `Cos[c x]/(a x)`, `Exp[c x]/(a x)`, `1/Log[x]`,
  `Exp[-x^2]`, rational functions with repeated linear factors and with a
  any power of an irreducible quadratic (`Integrate[1/(1 + x^2)^2, x]` →
  `1/2 x/(1 + x^2) + 1/2 ArcTan[x]`, and `1/(1 + x^2)^3` likewise, by the standard
  reduction, which recurses down to the arc tangent)
  (`Integrate[1/(x^2 (1 + x)), x]` → `Log[1 + x] - Log[x] - 1/x`, via the
  generalised `Apart`), and every product `Sin[u]^m Cos[u]^n` with integer
  exponents ≥ 0 — the substitution `t = sin u`/`t = cos u` when one exponent is
  odd, and the reduction `∫sin^m cos^n = sin^(m+1)cos^(n-1)/(m+n) + ((n-1)/(m+n))∫sin^m cos^(n-2)`
  when both are even (so `Integrate[Sin[x]^2 Cos[x]^2, x]` gives
  `x/8 - Cos[x] Sin[x]/8 + Cos[x] Sin[x]^3/4`, an antiderivative written with
  `sin u cos u` rather than `sin 2u`). A trig-power antiderivative is also not
  yet Lean-checked, because closing it needs the Pythagorean identity in the
  derivative tactic chain.
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
  bigfloat.rs   arbitrary-precision decimals (N[expr, prec])
  json.rs       minimal JSON parser/writer for the RPC
  lexer.rs      Mathematica tokenizer
  parser.rs     recursive-descent parser (full operator precedence)
  expr.rs       expression model + symbol table (attributes, own/down-values)
  eval.rs       standard evaluation loop, attributes, scoping
  pattern.rs    pattern matcher (Orderless/Flat aware)
  builtins/     registry + arithmetic, algebra, calculus, linalg, graphics…
  fmt.rs        OutputForm / InputForm / FullForm printers
  rpc.rs        JSON-RPC stdio server
  tex.rs        TeX / TraditionalForm printer used by the notebook
  main.rs       REPL, one-shot `-e`, `--server` and the app's CLI helpers
```

The evaluator follows Mathematica's evaluation semantics: evaluate the head,
evaluate arguments according to hold attributes, splice `Sequence`, strip
`Unevaluated`, apply user rules, then built-ins, then iterate until a
fixpoint (bounded by `$IterationLimit`/`$RecursionLimit`).

## Testing

```
cd rmath
cargo test            # 63 unit tests + 471 end-to-end golden cases
```

The Rust suite also includes a deterministic **fuzz** pass (`tests/fuzz.rs`:
token soup, mutated valid expressions and a grammar-based generator, asserting
no panic and a bounded evaluation) and **ordering property** tests
(`tests/ordering.rs`: the comparators are total orders and every printed result
re-parses to itself).

The Python front end has its own unit tests — the notebook server's routing,
helpers and served page, the SVG renderer, path/tool resolution, the harness
configuration and the Lean bridge's translation helpers. They need no Lean, no
network and no running server:

```
make py-test          # python3 -m unittest discover -s tests
```

`make check` is the fast, dependency-free set CI runs: `make test`,
`make py-test`, `make oracle` and `make lean-test`. The workflow
(`.github/workflows/ci.yml`) runs the kernel suite on Linux and the Python
suites against Python 3.9, 3.11 and 3.12.

## License

MIT — see `LICENSE`.