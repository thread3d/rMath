"""Render rMath Graphics JSON to a styled SVG (Mathematica-like)."""

import math


def _fmt_num(v: float) -> str:
    if v == int(v) and abs(v) < 1e16:
        return str(int(v))
    return ("%g" % v)


def _nice_step(span: float, target: float) -> float:
    if span <= 0:
        return 1.0
    raw = span / max(target, 1.0)
    mag = 10.0 ** math.floor(math.log10(raw))
    for mult in (1.0, 2.0, 5.0, 10.0):
        if raw <= mult * mag + 1e-12:
            return mult * mag
    return 10.0 * mag


def _ticks(lo: float, hi: float):
    span = hi - lo
    if span <= 0:
        return []
    step = _nice_step(span, 5.0)
    k = math.ceil(lo / step - 1e-9)
    out = []
    while k * step <= hi + step * 1e-9:
        v = k * step
        if lo - 1e-9 <= v <= hi + 1e-9:
            out.append(v)
        k += 1
    return out


def _label(v: float, step: float) -> str:
    v = round(v, 10)
    if abs(v) < step * 1e-9:
        v = 0.0
    decimals = max(0, -int(math.floor(math.log10(step if step > 0 else 1)))) 
    if decimals > 4:
        return "%g" % v
    return ("%.*f" % (decimals, v)).rstrip("0").rstrip(".") or "0"


def render_graphics(g: dict, width=360, height=240) -> str:
    prims = g.get("primitives", [])
    pr = g.get("plotrange")
    xs, ys = [], []
    for p in prims:
        pts = p.get("pts") or ([p["pt"]] if "pt" in p else [])
        for x, y in pts:
            if math.isfinite(x) and math.isfinite(y):
                xs.append(x)
                ys.append(y)
    # A ListPlot with no points (or one) has no bounds to report, and the kernel
    # sends `None` for them: unpacking that crashed the renderer with a TypeError
    # instead of drawing an empty frame.
    pr_ok = (isinstance(pr, (list, tuple)) and len(pr) == 4
             and all(isinstance(v, (int, float)) and math.isfinite(v) for v in pr))
    if pr_ok:
        xmin, xmax, ymin, ymax = (float(v) for v in pr)
    else:
        xmin = min(xs) if xs else 0.0
        xmax = max(xs) if xs else 1.0
        ymin = min(ys) if ys else 0.0
        ymax = max(ys) if ys else 1.0
        if xmax == xmin:
            xmax = xmin + 1.0
        if ymax == ymin:
            ymax = ymin + 1.0
    if ymax == ymin:
        ymax = ymin + 1.0
    if xmax == xmin:
        xmax = xmin + 1.0

    pad_l, pad_b, pad_t, pad_r = 44.0, 30.0, 12.0, 14.0
    plot_w = width - pad_l - pad_r
    plot_h = height - pad_t - pad_b

    def px(x):
        return pad_l + (x - xmin) / (xmax - xmin) * plot_w

    def py(y):
        return pad_t + plot_h - (y - ymin) / (ymax - ymin) * plot_h

    parts = []
    parts.append(
        '<svg xmlns="http://www.w3.org/2000/svg" width="%g" height="%g" '
        'viewBox="0 0 %g %g">' % (width, height, width, height)
    )
    parts.append('<rect width="100%" height="100%" fill="white"/>')
    parts.append(
        '<clipPath id="clip"><rect x="%g" y="%g" width="%g" height="%g"/></clipPath>'
        % (pad_l, pad_t, plot_w, plot_h)
    )

    # Axes (crossing at 0 when inside range, else at edges).
    ax_y = 0.0 if ymin <= 0 <= ymax else ymin
    ax_x = 0.0 if xmin <= 0 <= xmax else xmin
    ax_y_pix = py(ax_y)
    ax_x_pix = px(ax_x)

    parts.append('<g stroke="#b7b7b7" stroke-width="0.8" fill="none">')
    parts.append(
        '<line x1="%g" y1="%g" x2="%g" y2="%g"/>'
        % (pad_l, ax_y_pix, pad_l + plot_w, ax_y_pix)
    )
    parts.append(
        '<line x1="%g" y1="%g" x2="%g" y2="%g"/>'
        % (ax_x_pix, pad_t, ax_x_pix, pad_t + plot_h)
    )
    parts.append("</g>")

    # Ticks + labels
    label_style = (
        'font-family="Helvetica,Arial,sans-serif" font-size="10" fill="#444"'
    )
    for v in _ticks(xmin, xmax):
        x = px(v)
        parts.append(
            '<line x1="%g" y1="%g" x2="%g" y2="%g" stroke="#b7b7b7" stroke-width="0.8"/>'
            % (x, ax_y_pix, x, ax_y_pix + 3)
        )
        parts.append(
            '<text x="%g" y="%g" %s text-anchor="middle">%s</text>'
            % (x, ax_y_pix + 14, label_style, _label(v, _nice_step(xmax - xmin, 5.0)))
        )
    for v in _ticks(ymin, ymax):
        y = py(v)
        parts.append(
            '<line x1="%g" y1="%g" x2="%g" y2="%g" stroke="#b7b7b7" stroke-width="0.8"/>'
            % (ax_x_pix - 3, y, ax_x_pix, y)
        )
        parts.append(
            '<text x="%g" y="%g" %s text-anchor="end">%s</text>'
            % (ax_x_pix - 6, y + 3.5, label_style, _label(v, _nice_step(ymax - ymin, 5.0)))
        )

    # Plot border
    parts.append(
        '<rect x="%g" y="%g" width="%g" height="%g" fill="none" stroke="#d8d8d8" '
        'stroke-width="1"/>' % (pad_l, pad_t, plot_w, plot_h)
    )

    # Primitives (clipped)
    parts.append('<g clip-path="url(#clip)">')
    for p in prims:
        if p.get("type") == "Line":
            pts = p.get("pts") or []
            if len(pts) < 2:
                continue
            d = " ".join(
                "%s%g,%g" % ("M" if k == 0 else "L", px(pt[0]), py(pt[1]))
                for k, pt in enumerate(pts)
            )
            parts.append(
                '<path d="%s" fill="none" stroke="rgb(42,88,178)" stroke-width="1.4" '
                'stroke-linejoin="round" stroke-linecap="round"/>' % d
            )
        elif p.get("type") == "Point":
            pt = p["pt"]
            parts.append(
                '<circle cx="%g" cy="%g" r="2.5" fill="rgb(197,60,60)"/>'
                % (px(pt[0]), py(pt[1]))
            )
    parts.append("</g>")
    parts.append("</svg>")
    return "".join(parts)