#!/bin/bash
# rMath.app launcher — starts the bundled notebook server and opens it.
#
#   rMath                launch (server + browser), stay running until quit
#   rMath --no-open      launch without opening a browser (for testing)
#   rMath --port=9000    use a different port
#   rMath --stop         stop a running server and exit
#
# Quitting the app (Dock ▸ Quit, or Cmd-Q) stops the server and its kernel.

set -u

RES="$(cd "$(dirname "$0")/../Resources" && pwd)"
APP_SUPPORT="$HOME/Library/Application Support/rMath"
LOG_DIR="$HOME/Library/Logs"
# Fall back to a temp dir when the standard locations are not writable
# (e.g. a sandboxed launch), so the app still runs.
if ! mkdir -p "$APP_SUPPORT" 2>/dev/null; then
  APP_SUPPORT="${TMPDIR:-/tmp}/rMath-$(id -u)"
  mkdir -p "$APP_SUPPORT" 2>/dev/null || true
fi
if ! mkdir -p "$LOG_DIR" 2>/dev/null || ! : >>"$LOG_DIR/.rmath_write_test" 2>/dev/null; then
  LOG_DIR="${TMPDIR:-/tmp}"
fi
rm -f "$LOG_DIR/.rmath_write_test" 2>/dev/null || true
LOG="$LOG_DIR/rMath.log"
PIDFILE="$APP_SUPPORT/server.pid"

PORT="${RMATH_PORT:-8787}"
OPEN_BROWSER=1
STOP_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --no-open) OPEN_BROWSER=0 ;;
    --stop)    STOP_ONLY=1 ;;
    --port=*)  PORT="${arg#--port=}" ;;
  esac
done

mkdir -p "$APP_SUPPORT" "$LOG_DIR" 2>/dev/null || true

stop_server() {
  if [ -f "$PIDFILE" ]; then
    local pid
    pid="$(cat "$PIDFILE" 2>/dev/null || true)"
    if [ -n "${pid:-}" ] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
      sleep 0.5
      kill -9 "$pid" 2>/dev/null || true
    fi
    rm -f "$PIDFILE"
  fi
  pkill -f 'rmathpy\.notebook' 2>/dev/null || true
  pkill -f 'rmath --server' 2>/dev/null || true
}

if [ "$STOP_ONLY" = "1" ]; then
  stop_server
  echo "rMath stopped"
  exit 0
fi

is_up() {
  curl -s --max-time 1 "http://127.0.0.1:$1/api/health" >/dev/null 2>&1
}

find_python() {
  local cand
  for cand in "${RMATH_PYTHON:-}" \
              "$HOME/miniconda3/bin/python3" \
              /Users/threaded/miniconda3/bin/python3 \
              /opt/homebrew/bin/python3 \
              /usr/local/bin/python3 \
              /usr/bin/python3; do
    if [ -n "$cand" ] && [ -x "$cand" ] && \
       "$cand" -c 'import http.server, json, subprocess, threading' >/dev/null 2>&1; then
      echo "$cand"
      return 0
    fi
  done
  if command -v python3 >/dev/null 2>&1; then
    command -v python3
    return 0
  fi
  return 1
}

alert() {
  osascript -e "display alert \"rMath\" message \"$1\"" >/dev/null 2>&1 || true
}

# Already running? Just bring up the page.
if is_up "$PORT"; then
  [ "$OPEN_BROWSER" = "1" ] && open "http://127.0.0.1:$PORT"
  echo "rMath is already running at http://127.0.0.1:$PORT"
  exit 0
fi

PY="$(find_python)" || {
  alert "Could not find a usable python3. Install Python 3, or set RMATH_PYTHON."
  exit 1
}

# Kernel working directory: writable, holds a copy of the demo files.
WORKDIR="$APP_SUPPORT/work"
mkdir -p "$WORKDIR"
if [ -d "$RES/demo" ] && [ ! -d "$WORKDIR/demo" ]; then
  cp -R "$RES/demo" "$WORKDIR/demo" 2>/dev/null || true
fi

export RMATH_BIN="$RES/rmath"
export RMATH_WORKDIR="$WORKDIR"
export PYTHONPATH="$RES"
export PYTHONUNBUFFERED=1

{
  echo "--- rMath launch $(date) python=$PY port=$PORT workdir=$WORKDIR"
} >> "$LOG"

"$PY" -u -m rmathpy.notebook --port "$PORT" >> "$LOG" 2>&1 &
SERVER_PID=$!
echo "$SERVER_PID" > "$PIDFILE"

# Wait for the server to answer (up to ~10 s).
for _ in $(seq 1 100); do
  if is_up "$PORT"; then break; fi
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then break; fi
  sleep 0.1
done

if ! is_up "$PORT"; then
  alert "The notebook server failed to start. See ~/Library/Logs/rMath.log"
  rm -f "$PIDFILE"
  exit 1
fi

echo "rMath listening on http://127.0.0.1:$PORT"
[ "$OPEN_BROWSER" = "1" ] && open "http://127.0.0.1:$PORT"

cleanup() {
  kill "$SERVER_PID" 2>/dev/null || true
  sleep 0.3
  pkill -f 'rmath --server' 2>/dev/null || true
  rm -f "$PIDFILE"
  exit 0
}
trap cleanup TERM INT HUP

wait "$SERVER_PID"
rm -f "$PIDFILE"
