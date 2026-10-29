# rMath Kernel Protocol (JSON over stdio)

The Python front end spawns `rmath --server` and exchanges **newline-delimited
JSON** on stdin/stdout (one request per line, one response per line).

## Requests

```json
{"id": 1, "op": "eval",   "input": "D[Sin[x]^2, x]"}
{"id": 2, "op": "names"}
{"id": 3, "op": "reset"}
{"id": 4, "op": "shutdown"}
```

## Responses

```json
{"id": 1, "status": "ok", "out": 3,
 "result": "2 Cos[x] Sin[x]",
 "input_form": "2*Cos[x]*Sin[x]"}

{"id": 2, "status": "ok",
 "result": "Graphics[...]",
 "input_form": "...",
 "graphics": {
   "type": "Graphics",
   "primitives": [
     {"type": "Line", "pts": [[0.0, 0.0], [0.025, 0.025], ...]},
     {"type": "Point", "pt": [1.0, 2.0]}
   ],
   "plotrange": [xmin, xmax, ymin, ymax]
 },
 "prints": ["hello"],
 "messages": [{"tag": "General::dims", "text": "..."}]}

{"id": 3, "status": "error", "error": "Syntax error at line 1: ..."}
```

- `out` is present only when a non-Null result was produced (the In/Out counter).
- `graphics` is present only when the result is a `Graphics[...]` expression.
- `result` is OutputForm (ASCII infix, Mathematica-like).
- `input_form` is re-parseable functional notation (for copy/paste).
- `prints` contains text emitted by `Print[...]`.
- `messages` contains warnings/errors raised during evaluation.

## Graphics JSON schema

- `type`: always `"Graphics"`
- `primitives`: array of
  - `{"type": "Line", "pts": [[x, y], ...]}` (polyline; may be several)
  - `{"type": "Point", "pt": [x, y]}`
- `plotrange`: `[xmin, xmax, ymin, ymax]` — the axes bounds to draw
  (render with a small margin, axes at y=0/x=0 when inside range,
  otherwise at the lower/left edge, Mathematica-style).

## Notes

- Responses are strictly one line of JSON each; no interleaving.
- The kernel runs on a 512 MiB stack; deep recursion raises
  `$RecursionLimit::reclim` (an error response) rather than crashing.
- `reset` restores a pristine kernel (clears all user definitions).

## Evaluation limit

Each `eval` carries a wall-clock limit, because one runaway cell would otherwise
block the server forever — the notebook holds its kernel lock across an
evaluation, so the page, the Lean panel and the harness panel would all wait.

```json
{"id": 7, "op": "eval", "input": "Do[Null, {10^9}]", "timeout": 2.5}
```

- Default 60 s. `RMATH_EVAL_TIMEOUT` (seconds, in the environment) changes it for
  every request, and a per-request `"timeout"` overrides that. `0` disables it.
- When the limit is reached the evaluation is aborted, the response carries
  `"result": "$Aborted"` with a message naming the limit, and **the session
  survives**: the definitions made before the runaway cell are still there, and
  the next request evaluates normally. A deadline is never carried over to the
  next request.
- A loop that swallows the abort is a bug, not a feature: `Do` used to end
  silently and answer `Null` as if it had finished. `Break` and `Continue` still
  end their loop, but an abort propagates to the response.
