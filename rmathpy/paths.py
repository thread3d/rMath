"""Where the project lives, from the front end's point of view.

The front end can be run three ways: from the repository (`make notebook`), from
inside the app bundle (`/Applications/rMath.app`, where `__file__` points at
Contents/Resources and is the *wrong* answer), or under a caller that sets the
environment. These helpers resolve it once, in the right order, so the Lean
project and the harness workspace are the same repository in every case.
"""

import os


def _marker():
    """The path recorded by `make app-install` in the app-support directory."""
    path = os.path.expanduser("~/Library/Application Support/rMath/workspace")
    try:
        with open(path) as f:
            return f.read().strip()
    except OSError:
        return ""


def workspace_root():
    """The *repository*.

    Order matters: the app sets RMATH_WORKDIR to its own scratch copy, which is
    right for sandboxed file I/O but is not where the Lean project or the git
    checkout live. The path recorded by `make app-install` names the repository,
    so it comes first; an explicit override still wins over everything.
    """
    value = os.environ.get("RMATH_HARNESS_ROOT")
    if value and os.path.isdir(value):
        return value
    recorded = _marker()
    if recorded and os.path.isdir(recorded):
        return recorded
    value = os.environ.get("RMATH_WORKDIR")
    if value and os.path.isdir(value):
        return value
    return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def lean_project():
    """The Lean/Mathlib project: an explicit override, else `<repo>/lean`."""
    env = os.environ.get("RMATH_LEAN_PROJECT")
    if env and os.path.isdir(env):
        return env
    return os.path.join(workspace_root(), "lean")
