"""Kernel process client: spawns `rmath --server` and speaks JSON lines."""

import json
import os
import select
import subprocess
import threading


def find_kernel():
    """Locate the rmath kernel binary."""
    env = os.environ.get("RMATH_BIN")
    if env and os.path.isfile(env):
        return env
    here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    for cand in (
        os.path.join(here, "rmath", "target", "release", "rmath"),
        os.path.join(here, "rmath", "target", "debug", "rmath"),
    ):
        if os.path.isfile(cand):
            return cand
    return "rmath"


class Kernel:
    """A line-JSON RPC client around one `rmath --server` process."""

    def __init__(self, path=None):
        self.path = find_kernel()
        self._proc = None
        self._id = 0
        self._lock = threading.Lock()

    def _spawn(self):
        # Run the kernel from a writable working directory so relative paths
        # (e.g. Get["demo/demo.mt"]) work; RMATH_WORKDIR overrides it.
        root = os.environ.get("RMATH_WORKDIR") or os.path.dirname(
            os.path.dirname(os.path.abspath(__file__))
        )
        self._proc = subprocess.Popen(
            [self.path, "--server"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            bufsize=1,
            cwd=root,
        )

    def _ensure(self):
        if self._proc is None or self._proc.poll() is not None:
            self._spawn()

    def _kill(self):
        if self._proc is not None:
            try:
                self._proc.kill()
            except Exception:
                pass
            try:
                self._proc.wait(timeout=2)
            except Exception:
                pass
            self._proc = None

    def _timeout(self):
        """Seconds to wait for a response; 0 disables the limit."""
        try:
            t = float(os.environ.get("RMATH_EVAL_TIMEOUT", "120"))
        except ValueError:
            t = 120.0
        return t if t > 0 else 0

    def _read_line(self, timeout):
        """Return a response line, "" on EOF, or None if the wait timed out."""
        if timeout:
            try:
                ready, _, _ = select.select([self._proc.stdout], [], [], timeout)
            except Exception:
                return None
            if not ready:
                return None
        return self._proc.stdout.readline()

    def _roundtrip(self, body, timeout):
        payload = json.dumps(body) + "\n"
        for attempt in (0, 1):
            try:
                self._proc.stdin.write(payload)
                self._proc.stdin.flush()
                line = self._read_line(timeout)
            except Exception:
                line = ""
            if line is None or line:
                return line
            # Empty line: the kernel died. Respawn once and retry.
            if attempt == 0:
                self._spawn()
        return ""

    def _request(self, payload: dict) -> dict:
        with self._lock:
            self._ensure()
            self._id += 1
            body = dict(payload)
            body["id"] = self._id
            timeout = self._timeout()
            line = self._roundtrip(body, timeout)
            if line is None:
                # A runaway evaluation: killing the kernel is the only way out.
                self._kill()
                return {
                    "id": body["id"],
                    "status": "error",
                    "error": (
                        "Evaluation did not finish within %g s, so the kernel "
                        "was restarted (definitions from this session were "
                        "lost). Set RMATH_EVAL_TIMEOUT=0 to disable the limit."
                        % timeout
                    ),
                }
            if not line:
                self._spawn()
                return {
                    "id": body["id"],
                    "status": "error",
                    "error": "kernel stopped responding",
                }
            try:
                return json.loads(line)
            except Exception as e:
                return {
                    "id": body["id"],
                    "status": "error",
                    "error": "bad kernel response: %s" % e,
                }

    def eval(self, text: str, steps: bool = False) -> dict:
        return self._request({"op": "eval", "input": text, "steps": bool(steps)})

    def split(self, text: str):
        """Split a script into statements using the kernel's own splitter."""
        try:
            r = self._request({"op": "split", "input": text})
            return r.get("statements", [])
        except Exception:
            return []

    def names(self):
        try:
            r = self._request({"op": "names"})
            return r.get("names", [])
        except Exception:
            return []

    def reset(self):
        return self._request({"op": "reset"})

    def close(self):
        if self._proc is not None:
            try:
                self._proc.stdin.write(json.dumps({"op": "shutdown"}) + "\n")
                self._proc.stdin.flush()
            except Exception:
                pass
            try:
                self._proc.wait(timeout=2)
            except Exception:
                self._proc.kill()
            self._proc = None

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()