"""rMath Python front end: kernel client, SVG renderer, notebook server."""

import os as _os
import re as _re


def _project_version():
    """The project version, read from the crate manifest — the one place it is
    written down. `rmath --version`, `$Version` and the MCP server all report it,
    so a version that drifts between them is a bug, not a label."""
    here = _os.path.dirname(_os.path.abspath(__file__))
    try:
        with open(_os.path.join(here, "..", "rmath", "Cargo.toml")) as f:
            m = _re.search(r'^version = "([^"]+)"', f.read(), _re.M)
        return m.group(1) if m else "0.0.0"
    except OSError:
        return "0.0.0"


__version__ = _project_version()

from .kernel import Kernel, find_kernel
from .client import eval_svg
from .svg import render_graphics

__all__ = ["Kernel", "find_kernel", "eval_svg", "render_graphics", "__version__"]