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

# Make the check self-sufficient: if nothing is serving the URL, start a
# temporary notebook (and stop it again), instead of stalling on a dead port.
PORT="${URL##*:}"
STARTED=""
if ! curl -fsS --max-time 3 "$URL/api/health" >/dev/null 2>&1; then
  ( cd "$ROOT" && nohup python3 -m rmathpy.notebook --port "$PORT" >"$TMP/server.log" 2>&1 & )
  STARTED="1"
  for _ in $(seq 1 40); do
    curl -fsS --max-time 2 "$URL/api/health" >/dev/null 2>&1 && break
    sleep 0.5
  done
fi
stop_temp_server() {
  if [ -n "$STARTED" ]; then pkill -f "rmathpy.notebook --port $PORT" 2>/dev/null || true; fi
  rm -rf "$TMP"
}
trap stop_temp_server EXIT

[ -x "$SHOT" ] || make -C "$ROOT" scripts/dev/screenshot >/dev/null

SETUP="$(cat "$ROOT/scripts/dev/uitest-setup.js")"
COLLECT="$(cat "$ROOT/scripts/dev/uitest-collect.js")"

"$SHOT" "$URL" "$TMP/shot.png" 14 "$SETUP" "$TMP/result.json" "$COLLECT" >/dev/null

python3 - "$TMP/result.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
errs = d.get("errs") or []
cells = d.get("cells", 0)
st = d.get("styles") or {}
print("JS errors:", errs or "none")
print("cells after clicking Evaluate:", cells)
print("form after toggling and restoring:", d.get("form"),
      "(unchanged: %s)" % d.get("formUnchanged"))
print("first results:", d.get("results"))
style_problems = []
if not st.get("leanFixed") or st.get("leanWidth", 0) < 200:
    style_problems.append("Lean panel is not a floating panel: position/width %r/%r"
                          % (st.get("leanDisplay"), st.get("leanWidth")))
if not st.get("leanHeaderPadding") or st.get("leanHeaderPadding") == "0px":
    style_problems.append("Lean header has no padding (its rules are missing?)")
if not st.get("agentFixed") or st.get("agentWidth", 0) < 200:
    style_problems.append("Harness panel is not a floating panel: width %r" % st.get("agentWidth"))
if st.get("logBackground") not in ("rgb(234, 241, 248)", "rgba(0, 0, 0, 0)"):
    style_problems.append("log background changed: %r" % st.get("logBackground"))
if st.get("texFontSize") in (None, "16px") or not st.get("inLabelColor"):
    style_problems.append("notebook typography/labels lost their styling")
if st.get("ribbonButtons", 0) < 8:
    style_problems.append("ribbon lost buttons: %r" % st.get("ribbonButtons"))
for pb in style_problems:
    print("STYLE PROBLEM:", pb)
print("styles:", {k: st.get(k) for k in ("leanFixed", "leanWidth", "agentFixed", "agentWidth",
                                         "logBackground", "texFontSize", "ribbonButtons")})
ok = (not errs) and cells > 0 and d.get("formUnchanged") and not style_problems
print("UITEST: %s" % ("PASS" if ok else "FAIL"))
sys.exit(0 if ok else 1)
PY
