#!/usr/bin/env bash
# Load a file (or evaluate expressions) in a real WebKit page and check that
# every generated TeX string parses in KaTeX. Catches TeX printer bugs that
# would otherwise degrade to raw LaTeX in the notebook.
#
#   scripts/dev/texcheck.sh [url] [file-to-open] [wait-seconds]
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
URL="${1:-http://127.0.0.1:8787}"
FILE="${2:-demo/demo.mt}"
WAIT="${3:-75}"
SHOT="$ROOT/scripts/dev/screenshot"
TMP="$(mktemp -d)"

# Make the check self-sufficient: if nothing is serving the URL, start a
# temporary notebook (and stop it again), instead of stalling on a dead port.
PORT="${URL##*:}"
STARTED=""
if ! curl -fsS --max-time 3 "$URL/api/health" >/dev/null 2>&1; then
  # A throwaway work dir: rendering the demo autosaves a session, which must
  # not land in the repository. The demo itself is linked in so the relative
  # path `demo/demo.mt` still resolves.
  mkdir -p "$TMP/work"
  ln -sfn "$ROOT/demo" "$TMP/work/demo"
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

SETUP="window.confirm = () => true; newNotebook(); openNotebook(false, \"$FILE\");"
COLLECT="$(cat "$ROOT/scripts/dev/texcheck.js")"

"$SHOT" "$URL" "$TMP/shot.png" "$WAIT" "$SETUP" "$TMP/result.json" "$COLLECT" >/dev/null

python3 - "$TMP/result.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
spans, bad = d.get("spans", 0), d.get("bad") or []
print("typeset spans checked:", spans)
print("invalid TeX strings:", len(bad))
for b in bad:
    print("   ", b)
ok = spans > 0 and not bad
print("TEXCHECK: %s" % ("PASS" if ok else "FAIL"))
sys.exit(0 if ok else 1)
PY
