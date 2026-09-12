"""Kernel process client: spawns `rmath --server` and speaks JSON lines."""

import json
import os
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

    def _request(self, payload: dict) -> dict:
        with self._lock:
            self._ensure()
            self._id += 1
            body = dict(payload)
            body["id"] = self._id
            try:
                self._proc.stdin.write(json.dumps(body) + "\n")
                self._proc.stdin.flush()
                line = self._proc.stdout.readline()
            except Exception:
                self._spawn()
                self._proc.stdin.write(json.dumps(body) + "\n")
                self._proc.stdin.flush()
                line = self._proc.stdout.readline()
            if not line:
                # Kernel died; restart once and retry.
                self._spawn()
                self._proc.stdin.write(json.dumps(body) + "\n")
                self._proc.stdin.flush()
                line = self._proc.stdout.readline()
            return json.loads(line)

    def eval(self, text: str) -> dict:
        return self._request({"op": "eval", "input": text})

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