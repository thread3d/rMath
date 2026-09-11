"""rMath Python front end: kernel client, SVG renderer, notebook server."""

from .kernel import Kernel, find_kernel
from .client import eval_svg
from .svg import render_graphics

__all__ = ["Kernel", "find_kernel", "eval_svg", "render_graphics"]