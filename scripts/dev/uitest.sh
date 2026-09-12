#!/usr/bin/env bash
# Click the notebook's controls in a real WebKit page and fail on any JS error.
# Catches handler-wiring bugs (e.g. an inline onclick resolving to a DOM method
# such as document.evaluate instead of the intended global function).
#
#   scripts/dev/uitest.sh [url]
#
# Only non-destructive controls are exercised and the typesetting form is
# restored, so running this does not change the notebook state.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
URL="${1:-http://127.0.0.1:8787}"
SHOT="$ROOT/scripts/dev/screenshot"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

[ -x "$SHOT" ] || make -C "$ROOT" scripts/dev/screenshot >/dev/null

SETUP="$(cat "$ROOT/scripts/dev/uitest-setup.js")"
COLLECT="$(cat "$ROOT/scripts/dev/uitest-collect.js")"

"$SHOT" "$URL" "$TMP/shot.png" 14 "$SETUP" "$TMP/result.json" "$COLLECT" >/dev/null

python3 - "$TMP/result.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
errs = d.get("errs") or []
cells = d.get("cells", 0)
print("JS errors:", errs or "none")
print("cells after clicking Evaluate:", cells)
print("form after toggling and restoring:", d.get("form"),
      "(unchanged: %s)" % d.get("formUnchanged"))
print("first results:", d.get("results"))
ok = (not errs) and cells > 0 and d.get("formUnchanged")
print("UITEST: %s" % ("PASS" if ok else "FAIL"))
sys.exit(0 if ok else 1)
PY
