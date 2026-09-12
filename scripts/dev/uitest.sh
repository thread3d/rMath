#!/usr/bin/env bash
# Click every notebook control in a real WebKit page and fail on any JS error.
# Catches handler-wiring bugs (e.g. an inline onclick resolving to a DOM method
# such as document.evaluate instead of the intended global function).
#
#   scripts/dev/uitest.sh [url]
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
URL="${1:-http://127.0.0.1:8787}"
SHOT="$ROOT/scripts/dev/screenshot"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

[ -x "$SHOT" ] || { echo "building screenshot tool"; make -C "$ROOT" scripts/dev/screenshot >/dev/null; }

JS_SETUP='window.__errs=[]; window.addEventListener("error", e => window.__errs.push((e.message||"")+" @ "+String((e.error&&e.error.stack)||"").split("\n")[1])); document.getElementById("inp").value="1 + 1\nFactor[x^2 - 1]"; document.getElementById("go").click(); setTimeout(()=>{document.querySelectorAll("#ribbon .tb").forEach(b=>b.click());},2500);'
"$SHOT" "$URL" "$TMP/shot.png" 14 "$JS_SETUP" "$TMP/result.json" \
  'JSON.stringify({errs: window.__errs, cells: document.querySelectorAll(".cell").length, results: [...document.querySelectorAll(".outrow .content")].map(e=>e.textContent.trim()).slice(0,4)})' >/dev/null

python3 - "$TMP/result.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
errs, cells = d.get("errs") or [], d.get("cells", 0)
print("JS errors:", errs or "none")
print("cells after clicking Evaluate:", cells)
print("first results:", d.get("results"))
if errs or cells == 0:
    print("UITEST: FAIL")
    sys.exit(1)
print("UITEST: PASS")
PY
