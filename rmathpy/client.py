"""Thin programmatic client helpers."""

from .kernel import Kernel
from .svg import render_graphics

__all__ = ["eval_svg"]


def eval_svg(k: Kernel, text: str):
    """Evaluate text; return (kernel_response, svg_or_None)."""
    r = k.eval(text)
    svg = render_graphics(r["graphics"]) if r.get("graphics") else None
    return r, svg