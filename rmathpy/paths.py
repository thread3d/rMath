"""Where the project lives, from the front end's point of view.

The front end can be run three ways: from the repository (`make notebook`), from
inside the app bundle (`/Applications/rMath.app`, where `__file__` points at
Contents/Resources and is the *wrong* answer), or under a caller that sets the
environment. These helpers resolve it once, in the right order, so the Lean
project and the harness workspace are the same repository in every case.
"""

import os
import shutil


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


def common_bin_dirs():
    """Directories where a user's tools usually live.

    A GUI app launches its helper with the minimal system PATH, so tools
    installed under the home directory (node via npm or nvm, elan, conda) are
    invisible unless they are added explicitly. `dsh` is a node script, so
    without this the harness fails with `env: node: No such file or directory`.
    """
    home = os.path.expanduser("~")
    dirs = [
        os.path.join(home, ".npm-packages/bin"),
        os.path.join(home, ".local/bin"),
        os.path.join(home, ".elan/bin"),
        os.path.join(home, ".cargo/bin"),
        os.path.join(home, "miniconda3/bin"),
        os.path.join(home, "anaconda3/bin"),
        "/opt/homebrew/bin",
        "/usr/local/bin",
    ]
    nvm = os.path.join(home, ".nvm/versions/node")
    if os.path.isdir(nvm):
        for version in sorted(os.listdir(nvm), reverse=True):
            dirs.append(os.path.join(nvm, version, "bin"))
    return dirs


def find_tool(name):
    """Locate an executable on PATH or in the common directories."""
    found = shutil.which(name)
    if found:
        return found
    for d in common_bin_dirs():
        cand = os.path.join(d, name)
        if os.path.isfile(cand) and os.access(cand, os.X_OK):
            return cand
    return None


def child_env(extra_path=()):
    """An environment for a helper process, with a usable PATH.

    Keeps the inherited PATH but puts the common directories (and any directory
    holding a resolved `node` or `dsh`) in front, so a bundled app can run the
    same tools a terminal can.
    """
    env = dict(os.environ)
    parts = [p for p in extra_path if p and os.path.isdir(p)]
    parts += [d for d in common_bin_dirs() if os.path.isdir(d)]
    for tool in ("node", "dsh", "lake", "lean"):
        found = find_tool(tool)
        if found:
            d = os.path.dirname(found)
            if d not in parts:
                parts.insert(0, d)
    inherited = env.get("PATH", "")
    env["PATH"] = ":".join(parts + ([inherited] if inherited else []))
    return env


def preflight():
    """Tools the long-running features need, and whether they are reachable."""
    node = find_tool("node")
    dsh = find_tool("dsh")
    lake = find_tool("lake")
    problems = []
    if dsh and not node:
        problems.append("dsh needs node, which is not on PATH or in the usual "
                        "places (%s)" % ", ".join(common_bin_dirs()[:4]))
    return {"node": node, "dsh": dsh, "lake": lake, "problems": problems}
