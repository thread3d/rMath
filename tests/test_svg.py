"""Unit tests for the SVG plot renderer."""

import math
import unittest

from rmathpy import svg


class Numbers(unittest.TestCase):
    def test_fmt_num_hides_integral_floats(self):
        self.assertEqual(svg._fmt_num(3.0), "3")
        self.assertEqual(svg._fmt_num(3.5), "3.5")

    def test_nice_step(self):
        self.assertEqual(svg._nice_step(10.0, 5.0), 2.0)
        self.assertEqual(svg._nice_step(0.0, 5.0), 1.0)

    def test_ticks_land_inside_the_range(self):
        ticks = svg._ticks(0.0, 10.0)
        self.assertIn(0.0, ticks)
        self.assertIn(10.0, ticks)
        self.assertTrue(all(0.0 <= t <= 10.0 for t in ticks))
        self.assertEqual(svg._ticks(5.0, 5.0), [])

    def test_label_of_zero(self):
        self.assertEqual(svg._label(0.0, 1.0), "0")


class Render(unittest.TestCase):
    def test_line_primitive_produces_a_path(self):
        out = svg.render_graphics({
            "type": "Graphics",
            "primitives": [{"type": "Line", "pts": [[0, 0], [0.5, 0.5], [1, 1]]}],
            "plotrange": [0, 1, 0, 1],
        })
        self.assertIn("<svg", out)
        self.assertIn("</svg>", out)
        self.assertIn("<path", out)
        self.assertIn("rgb(42,88,178)", out)

    def test_point_primitive_produces_a_circle(self):
        out = svg.render_graphics({
            "primitives": [{"type": "Point", "pt": [1, 2]}],
            "plotrange": [0, 2, 0, 3],
        })
        self.assertIn("<circle", out)

    def test_empty_graphics_still_renders(self):
        out = svg.render_graphics({"primitives": []})
        self.assertIn("<svg", out)
        self.assertIn("</svg>", out)

    def test_non_finite_points_do_not_crash(self):
        out = svg.render_graphics({
            "primitives": [{"type": "Line",
                            "pts": [[0, 0], [math.inf, 1], [1, 1]]}],
        })
        self.assertIn("<svg", out)

    def test_plotrange_drives_the_axes(self):
        out = svg.render_graphics({
            "primitives": [{"type": "Line", "pts": [[2, 3], [4, 5]]}],
            "plotrange": [2, 4, 3, 5],
        })
        # A tick label from the y-range should appear.
        self.assertIn("<text", out)


if __name__ == "__main__":
    unittest.main()


class DegeneratePlotRange(unittest.TestCase):
    """A plot with no data has no bounds, and the kernel reports them as null.

    Unpacking that raised `TypeError: unsupported operand type(s) for +: 'NoneType'
    and 'float'` inside the renderer, so `/api/eval` of `ListPlot[{}]` failed
    instead of returning the cell's result.
    """

    def test_null_bounds_fall_back_to_the_data(self):
        from rmathpy.svg import render_graphics
        svg = render_graphics({"type": "Graphics", "primitives": [],
                               "plotrange": [None, None, None, None]})
        self.assertIn("<svg", svg)
        self.assertNotIn("nan", svg.lower())

    def test_a_single_point_still_draws_a_frame(self):
        from rmathpy.svg import render_graphics
        svg = render_graphics({"type": "Graphics",
                               "primitives": [{"type": "Point", "pt": [1.0, 2.0]}],
                               "plotrange": [None, None, None, None]})
        self.assertIn("<svg", svg)
        self.assertNotIn("nan", svg.lower())
