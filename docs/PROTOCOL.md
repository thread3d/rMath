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