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
  # A throwaway working directory: the test clicks Save and autosaves the
  # session, and those files must not land in the repository.
  mkdir -p "$TMP/work"
  ( cd "$ROOT" && RMATH_WORKDIR="$TMP/work" nohup python3 -m rmathpy.notebook --port "$PORT" >"$TMP/server.log" 2>&1 & )
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

"$SHOT" "$URL" "$TMP/shot.png" 20 "$SETUP" "$TMP/result.json" "$COLLECT" >/dev/null

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
if not d.get("steps"):
    style_problems.append("Show-steps toggle produced no derivation lines")
if not d.get("stepsRestored"):
    style_problems.append("Show-steps toggle did not restore its setting")
_ml = d.get("multiLineInput") or ""
if "N[Gamma[" not in _ml or "21]]" not in _ml:
    style_problems.append("a multi-line expression was split across cells: %r" % _ml)
if (d.get("agentModels") or 0) < 1:
    style_problems.append("the harness Model dropdown is empty")
if "D[Sin[x]^2, x]" not in (d.get("copyText") or ""):
    style_problems.append("copying a typeset cell did not yield its source: %r"
                          % d.get("copyText"))
if len(d.get("sessionInputs") or []) < 1:
    style_problems.append("the notebook session was not autosaved: %r"
                          % (d.get("sessionInputs") or [])[:3])
print("show-steps derivation lines:", d.get("steps"),
      "(setting restored: %s)" % d.get("stepsRestored"))
print("multi-line expression kept in one cell:", repr(d.get("multiLineInput")))
for pb in style_problems:
    print("STYLE PROBLEM:", pb)
print("styles:", {k: st.get(k) for k in ("leanFixed", "leanWidth", "agentFixed", "agentWidth",
                                         "logBackground", "texFontSize", "ribbonButtons")})
ok = (not errs) and cells > 0 and d.get("formUnchanged") and not style_problems
print("UITEST: %s" % ("PASS" if ok else "FAIL"))
sys.exit(0 if ok else 1)
PY
