#!/usr/bin/env python3
"""Draw the README's worked 3D example of Spherra's compression and scoring math.

The real index works in 768 dimensions. This script runs the same steps on a
3D toy vector so each stage can be drawn: normalize, randomly rotate, snap each
coordinate to a small grid, and fix the leftover with a shared codebook. The
toy grid has 4 levels (2 bits) instead of 16 (4 bits) and the codebook has 4
entries instead of 256, so the errors are large enough to see.

Run with no arguments; it rewrites the SVGs in docs/images and prints the
numbers quoted in the README. Standard library only.
"""

from __future__ import annotations

import math
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "docs" / "images"

# ---- The worked example ---------------------------------------------------

X = (4.0, 1.0, 0.5)  # an indexed vector with one dominant coordinate
QUERY = (3.0, 2.0, 1.0)
SIGNS = (1.0, -1.0, -1.0)
PERMUTATION = (1, 2, 0)  # new coordinate i reads old coordinate PERMUTATION[i]
LEVELS = (-0.75, -0.25, 0.25, 0.75)  # the toy "2-bit" grid for every coordinate
CODEBOOK = (
    (0.2, 0.2, -0.2),
    (-0.2, 0.2, 0.2),
    (0.2, -0.2, 0.2),
    (-0.2, -0.2, -0.2),
)


def dot(a, b):
    return sum(p * q for p, q in zip(a, b))


def norm(a):
    return math.sqrt(dot(a, a))


def add(a, b):
    return tuple(p + q for p, q in zip(a, b))


def sub(a, b):
    return tuple(p - q for p, q in zip(a, b))


def scale(a, s):
    return tuple(p * s for p in a)


def mix(v):
    """The 3D stand-in for a normalized Hadamard block: H = I - (2/3)J.

    Like Hadamard, H is orthogonal and its own inverse, and every output
    coordinate blends every input coordinate.
    """
    total = sum(v)
    return tuple(c - 2.0 * total / 3.0 for c in v)


def rotate(v):
    flipped = tuple(c * s for c, s in zip(v, SIGNS))
    permuted = tuple(flipped[source] for source in PERMUTATION)
    return mix(permuted)


def snap(value):
    return min(LEVELS, key=lambda level: abs(value - level))


def example():
    u = scale(X, 1.0 / norm(X))
    y = rotate(u)
    codes = tuple(LEVELS.index(snap(c)) for c in y)
    p = tuple(LEVELS[code] for code in codes)
    e = sub(y, p)
    centroid = min(range(len(CODEBOOK)), key=lambda i: norm(sub(e, CODEBOOK[i])))
    e_hat = CODEBOOK[centroid]
    r = add(p, e_hat)
    q = rotate(scale(QUERY, 1.0 / norm(QUERY)))
    return {
        "u": u,
        "y": y,
        "codes": codes,
        "p": p,
        "e": e,
        "centroid": centroid,
        "e_hat": e_hat,
        "r": r,
        "q": q,
        "truth": dot(scale(QUERY, 1.0 / norm(QUERY)), u),
        "truth_rotated": dot(q, y),
        "primary": dot(q, p),
        "refined": dot(q, r),
        "corrected": dot(q, r) / norm(r),
    }


# ---- Drawing --------------------------------------------------------------

THEMES = {
    "light": {
        "surface": "#fcfcfb",
        "text": "#0b0b0b",
        "text2": "#52514e",
        "grid": "#d6d5d0",
        "blue": "#2a78d6",
        "orange": "#eb6834",
        "aqua": "#1baf7a",
    },
    "dark": {
        "surface": "#1a1a19",
        "text": "#ffffff",
        "text2": "#c3c2b7",
        "grid": "#3d3d3a",
        "blue": "#3987e5",
        "orange": "#d95926",
        "aqua": "#199e70",
    },
}
FONT = "-apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, Arial, sans-serif"
AZIMUTH = math.radians(125)
ELEVATION = math.radians(20)


def fmt(v, digits=2):
    return "(" + ", ".join(f"{c:.{digits}f}" for c in v) + ")"


class Canvas:
    def __init__(self, width, height, theme):
        self.t = THEMES[theme]
        self.width = width
        self.height = height
        self.parts = []

    def raw(self, text):
        self.parts.append(text)

    def text(self, x, y, body, size=13, color="text", weight=400, anchor="start", italic=False):
        style = ' font-style="italic"' if italic else ""
        self.raw(
            f'<text x="{x:.1f}" y="{y:.1f}" font-size="{size}" font-weight="{weight}" '
            f'fill="{self.t[color]}" text-anchor="{anchor}"{style}>{body}</text>'
        )

    def line(self, a, b, color="grid", width=1.0, dash=None, arrow=False):
        dashed = f' stroke-dasharray="{dash}"' if dash else ""
        head = f' marker-end="url(#head-{color})"' if arrow else ""
        self.raw(
            f'<line x1="{a[0]:.1f}" y1="{a[1]:.1f}" x2="{b[0]:.1f}" y2="{b[1]:.1f}" '
            f'stroke="{self.t[color]}" stroke-width="{width}" stroke-linecap="round"{dashed}{head}/>'
        )

    def dot(self, c, radius, color, hollow=False):
        if hollow:
            paint = f'fill="{self.t["surface"]}" stroke="{self.t[color]}" stroke-width="2"'
        else:
            paint = f'fill="{self.t[color]}" stroke="{self.t["surface"]}" stroke-width="2"'
        self.raw(f'<circle cx="{c[0]:.1f}" cy="{c[1]:.1f}" r="{radius}" {paint}/>')

    def svg(self, title):
        heads = "".join(
            f'<marker id="head-{name}" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" '
            f'markerHeight="7" orient="auto-start-reverse"><path d="M0,1 L9,5 L0,9 z" '
            f'fill="{self.t[name]}"/></marker>'
            for name in ("text", "text2", "blue", "orange", "aqua")
        )
        return (
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{self.width}" height="{self.height}" '
            f'viewBox="0 0 {self.width} {self.height}" font-family="{FONT}" role="img">'
            f"<title>{title}</title><defs>{heads}</defs>"
            f'<rect width="100%" height="100%" rx="12" fill="{self.t["surface"]}"/>'
            + "".join(self.parts)
            + "</svg>\n"
        )


class Axes3D:
    """Orthographic view of 3D space inside one panel."""

    def __init__(self, canvas, cx, cy, size, focus=(0.0, 0.0, 0.0), azimuth=AZIMUTH):
        self.c = canvas
        self.cx = cx
        self.cy = cy
        self.size = size
        self.focus = focus  # the 3D point drawn at (cx, cy); used to zoom in
        self.azimuth = azimuth

    def at(self, v):
        x, y, z = sub(v, self.focus)
        x1 = x * math.cos(self.azimuth) - y * math.sin(self.azimuth)
        y1 = x * math.sin(self.azimuth) + y * math.cos(self.azimuth)
        up = z * math.cos(ELEVATION) - y1 * math.sin(ELEVATION)
        return (self.cx + self.size * x1, self.cy - self.size * up)

    def frame(self, sphere=True):
        origin = self.at((0, 0, 0))
        for axis in range(3):
            end = [0.0, 0.0, 0.0]
            end[axis] = 1.15
            start = [0.0, 0.0, 0.0]
            start[axis] = -1.0
            self.c.line(self.at(start), self.at(end), color="grid", width=1.2)
            label = [0.0, 0.0, 0.0]
            label[axis] = 1.27
            lx, ly = self.at(label)
            self.c.text(lx, ly + 4, f"c{'₁₂₃'[axis]}", size=12, color="text2", anchor="middle")
        if sphere:
            ring = [self.at((math.cos(a), math.sin(a), 0)) for a in _angles(72)]
            self.c.raw(_polyline(ring, self.c.t["grid"], dash="3 4"))
            self.c.raw(
                f'<circle cx="{origin[0]:.1f}" cy="{origin[1]:.1f}" r="{self.size}" fill="none" '
                f'stroke="{self.c.t["grid"]}" stroke-width="1"/>'
            )

    def cell(self, low, high, color="text2"):
        """Edges of the grid box whose corners are the candidate values of p."""
        corners = [
            (low[0] if i & 1 == 0 else high[0], low[1] if i & 2 == 0 else high[1], low[2] if i & 4 == 0 else high[2])
            for i in range(8)
        ]
        for i in range(8):
            for bit in (1, 2, 4):
                if i & bit == 0:
                    self.c.line(self.at(corners[i]), self.at(corners[i | bit]), color=color, width=0.8, dash="2 3")
        return corners

    def shadow(self, v):
        """Dashed drop-line to the c1-c2 floor, so depth is readable."""
        floor = (v[0], v[1], 0.0)
        self.c.line(self.at(floor), self.at(v), color="grid", width=1, dash="2 3")
        self.c.line(self.at((0, 0, 0)), self.at(floor), color="grid", width=1, dash="2 3")

    def arrow(self, v, color, width=2.0, start=(0, 0, 0), dash=None):
        self.c.line(self.at(start), self.at(v), color=color, width=width, dash=dash, arrow=True)

    def label(self, v, body, color, dx=8, dy=-8, anchor="start", size=13, weight=600):
        x, y = self.at(v)
        self.c.text(x + dx, y + dy, body, size=size, color=color, anchor=anchor, weight=weight)


def _angles(steps):
    return [2 * math.pi * i / steps for i in range(steps + 1)]


def _polyline(points, color, dash=None):
    dashed = f' stroke-dasharray="{dash}"' if dash else ""
    coords = " ".join(f"{x:.1f},{y:.1f}" for x, y in points)
    return f'<polyline points="{coords}" fill="none" stroke="{color}" stroke-width="1"{dashed}/>'


def coordinate_bars(canvas, x, y, values, color, caption):
    """Three small bars of |coordinate|; a flat profile is what the grid wants."""
    canvas.text(x, y, caption, size=12, color="text2")
    for i, value in enumerate(values):
        top = y + 12 + i * 17
        canvas.text(x, top + 10, f"c{'₁₂₃'[i]}", size=11, color="text2")
        width = 120 * abs(value)
        canvas.raw(
            f'<rect x="{x + 24}" y="{top}" width="{width:.1f}" height="11" rx="3" '
            f'fill="{canvas.t[color]}"/>'
        )
        canvas.text(x + 30 + width, top + 10, f"{abs(value):.2f}", size=11, color="text")


def panel_header(canvas, x, y, number, title, formula):
    canvas.text(x, y, f"{number}  {title}", size=16, weight=700)
    canvas.text(x, y + 22, formula, size=13, color="text2")


def compression_figure(theme, ex):
    c = Canvas(960, 950, theme)
    w = 480

    # 1. Normalize
    panel_header(c, 24, 36, "1", "Keep the direction", "u = x / ‖x‖   ·   ‖x‖ is stored separately (FP16)")
    a = Axes3D(c, 250, 240, 150)
    a.frame()
    x_drawn = scale(X, 0.3)
    a.shadow(x_drawn)
    a.arrow(x_drawn, "text2", width=1.6, dash="5 4")
    a.label(x_drawn, "x (drawn at 0.3×)", "text2", dx=4, dy=24, weight=400)
    a.shadow(ex["u"])
    a.arrow(ex["u"], "blue", width=2.5)
    a.label(ex["u"], "u", "blue", dx=6, dy=-6)
    c.text(24, 418, f"x = {fmt(X, 1)}   ‖x‖ = {norm(X):.3f}", size=13)
    c.text(24, 438, f"u = {fmt(ex['u'], 3)}   ‖u‖ = 1", size=13)

    # 2. Rotate
    panel_header(c, w + 24, 36, "2", "Randomly rotate", "y = H·P·S·u   ·   flip signs, shuffle, mix")
    a = Axes3D(c, w + 250, 240, 150)
    a.frame()
    a.arrow(ex["u"], "text2", width=1.4, dash="5 4")
    a.label(ex["u"], "u", "text2", dx=6, dy=-6, weight=400)
    a.shadow(ex["y"])
    a.arrow(ex["y"], "blue", width=2.5)
    a.label(ex["y"], "y", "blue", dx=8, dy=-4)
    coordinate_bars(c, w + 24, 380, ex["u"], "text2", "|u| per coordinate")
    coordinate_bars(c, w + 250, 380, ex["y"], "blue", "|y| per coordinate")

    # The grid box around y: each coordinate lies between two neighbouring levels.
    low = tuple(max(l for l in LEVELS if l <= c_) for c_ in ex["y"])
    high = tuple(min(l for l in LEVELS if l >= c_) for c_ in ex["y"])

    # 3. Snap to grid
    top = 470
    panel_header(c, 24, top + 36, "3", "Snap each coordinate to a grid", "p = nearest grid level, per coordinate")
    a = Axes3D(c, 250, top + 250, 150)
    a.frame(sphere=False)
    for gx in LEVELS:
        for gy in LEVELS:
            for gz in LEVELS:
                a.c.dot(a.at((gx, gy, gz)), 2.2, "text2")
    a.cell(low, high)
    a.arrow(ex["y"], "blue", width=2.5)
    a.label(ex["y"], "y", "blue", dx=-10, dy=-2, anchor="end")
    c.dot(a.at(ex["p"]), 5.5, "orange")
    a.label(ex["p"], "p", "orange", dx=8, dy=-6)
    c.text(24, top + 440, f"grid levels {LEVELS}  →  codes {ex['codes']}", size=13)
    c.text(24, top + 460, f"y = {fmt(ex['y'])}   p = {fmt(ex['p'])}", size=13)

    # 4. Fix the leftover (zoomed into the grid box from panel 3)
    panel_header(c, w + 24, top + 36, "4", "Fix the leftover with a codebook", "r = p + ê   ·   ê = codebook entry nearest to e = y − p")
    # Zoom on p and turn the view so y and r do not overlap on screen.
    focus = add(ex["p"], scale(ex["e_hat"], 0.5))
    a = Axes3D(c, w + 240, top + 255, 380, focus=focus, azimuth=math.radians(20))
    c.text(w + 24, top + 84, "zoomed in · hollow circles = other codebook entries", size=12, color="text2", italic=True)
    for index, entry in enumerate(CODEBOOK):
        if index != ex["centroid"]:
            c.dot(a.at(add(ex["p"], entry)), 5, "aqua", hollow=True)
    a.arrow(ex["y"], "orange", width=1.8, start=ex["p"], dash="4 3")
    a.label(add(ex["p"], scale(ex["e"], 0.55)), "e", "orange", dx=-12, dy=4, anchor="end")
    a.arrow(ex["r"], "aqua", width=2.2, start=ex["p"])
    a.label(add(ex["p"], scale(ex["e_hat"], 0.55)), "ê", "aqua", dx=10, dy=12)
    c.dot(a.at(ex["p"]), 5.5, "orange")
    a.label(ex["p"], "p", "orange", dx=-10, dy=-6, anchor="end")
    c.dot(a.at(ex["y"]), 5.5, "blue")
    a.label(ex["y"], "y", "blue", dx=10, dy=-6)
    c.dot(a.at(ex["r"]), 5.5, "aqua")
    a.label(ex["r"], "r", "aqua", dx=10, dy=16)
    c.text(w + 24, top + 440, f"ê = {fmt(ex['e_hat'], 1)}   r = {fmt(ex['r'])}", size=13)
    c.text(
        w + 24,
        top + 460,
        f"error ‖y − p‖ = {norm(ex['e']):.3f}  →  ‖y − r‖ = {norm(sub(ex['y'], ex['r'])):.3f}",
        size=13,
    )
    c.line((24, top - 8), (936, top - 8), color="grid")
    c.line((w, 20), (w, top + 470), color="grid")
    return c.svg("Four steps that compress a 3D vector")


def search_figure(theme, ex):
    c = Canvas(960, 460, theme)
    panel_header(c, 24, 36, "5", "Score a query", "rotation keeps dot products, so q·y is the true cosine")
    a = Axes3D(c, 250, 250, 150)
    a.frame()
    a.arrow(ex["q"], "text", width=2.2)
    a.label(ex["q"], "q", "text", dx=-10, dy=-2, anchor="end")
    a.arrow(ex["y"], "blue", width=2.2)
    a.label(ex["y"], "y", "blue", dx=8, dy=-6)
    a.arrow(ex["p"], "orange", width=1.6, dash="4 3")
    a.label(ex["p"], "p", "orange", dx=8, dy=-4)
    a.arrow(ex["r"], "aqua", width=2.2)
    a.label(ex["r"], "r", "aqua", dx=10, dy=14)
    c.text(24, 436, f"q ={fmt(ex['q'])}  (the query, normalized and rotated the same way)", size=13)

    rows = [
        ("True cosine", "q·y", ex["truth"], "blue", 1.0),
        ("Fast scan", "q·p", ex["primary"], "orange", 1.0),
        ("Refined", "q·r", ex["refined"], "aqua", 0.45),
        ("Length-corrected", "q·r / ‖r‖", ex["corrected"], "aqua", 1.0),
    ]
    left = 640
    bar_max = 240
    scale_top = max(value for *_, value, _, _ in rows)
    top_y = 110
    for i, (name, formula, value, color, opacity) in enumerate(rows):
        yy = top_y + i * 62
        c.text(left - 14, yy + 2, name, size=14, weight=600, anchor="end")
        c.text(left - 14, yy + 20, formula, size=12, color="text2", anchor="end")
        width = bar_max * value / scale_top
        c.raw(
            f'<rect x="{left}" y="{yy - 10}" width="{width:.1f}" height="26" rx="4" '
            f'fill="{c.t[color]}" fill-opacity="{opacity}"/>'
        )
        truth_x = left + bar_max * ex["truth"] / scale_top
        c.text(max(left + width, truth_x) + 8, yy + 8, f"{value:.3f}", size=14, weight=600)
    c.line((truth_x, top_y - 26), (truth_x, top_y + 3 * 62 + 24), color="text2", width=1.2, dash="4 3")
    c.text(truth_x, top_y - 32, "truth", size=12, color="text2", anchor="middle")
    return c.svg("Scoring a query against the compressed vector")


STEP_SECONDS = 2.4  # one stage: a move, then a pause to read it
MOVE_SECONDS = 0.9


def animation_figure(theme, ex):
    """An animated SVG (SMIL) of the vector passing through every transform.

    The projection is linear, so moving the arrow tip in a straight line on
    screen matches moving the vector in a straight line in 3D.
    """
    u = ex["u"]
    flipped = tuple(c_ * s for c_, s in zip(u, SIGNS))
    permuted = tuple(flipped[source] for source in PERMUTATION)
    stages = [
        (scale(X, 0.3), X, "text2", "Start", "x = (4, 1, 0.5), drawn at 0.3× size to fit"),
        (u, u, "blue", "Normalize", "u = x / ‖x‖: length 4.153 becomes 1"),
        (flipped, flipped, "blue", "Flip signs", "S·u: multiply by (+1, −1, −1)"),
        (permuted, permuted, "blue", "Shuffle", "P·S·u: each coordinate moves to a new slot"),
        (ex["y"], ex["y"], "blue", "Mix", "y = H·P·S·u: coordinates blend and even out"),
        (ex["p"], ex["p"], "orange", "Snap to grid", "p: each coordinate rounds to a grid level"),
        (ex["r"], ex["r"], "aqua", "Add correction", "r = p + ê: a codebook entry fixes the leftover"),
    ]
    n = len(stages)
    total = n * STEP_SECONDS
    c = Canvas(960, 420, theme)
    a = Axes3D(c, 250, 225, 150)
    a.frame()

    def discrete(attr, values):
        times = ";".join(f"{k / n:.4f}" for k in range(n))
        return (
            f'<animate attributeName="{attr}" dur="{total}s" repeatCount="indefinite" '
            f'calcMode="discrete" keyTimes="{times}" values="{";".join(str(v) for v in values)}"/>'
        )

    def moving(attr, per_stage):
        """Hold each stage's value, gliding to the next over MOVE_SECONDS."""
        frames = [(0.0, per_stage[0])]
        for k in range(1, n):
            start = k * STEP_SECONDS
            frames += [(start, per_stage[k - 1]), (start + MOVE_SECONDS, per_stage[k])]
        frames.append((total, per_stage[-1]))
        times = ";".join(f"{t / total:.4f}" for t, _ in frames)
        values = ";".join(f"{v:.1f}" for _, v in frames)
        return (
            f'<animate attributeName="{attr}" dur="{total}s" repeatCount="indefinite" '
            f'keyTimes="{times}" values="{values}"/>'
        )

    # The grid only matters once the vector snaps to it.
    grid_on = discrete("opacity", [1 if k >= 5 else 0 for k in range(n)])
    dots = "".join(
        f'<circle cx="{a.at((gx, gy, gz))[0]:.1f}" cy="{a.at((gx, gy, gz))[1]:.1f}" r="2" fill="{c.t["text2"]}"/>'
        for gx in LEVELS
        for gy in LEVELS
        for gz in LEVELS
    )
    c.raw(f'<g opacity="0">{grid_on}{dots}</g>')

    screen = [a.at(vector) for vector, *_ in stages]
    origin = a.at((0, 0, 0))
    colors = [c.t[color] for _, _, color, _, _ in stages]

    # A dashed ghost keeps the previous stage visible for comparison.
    ghost = [origin] + screen[:-1]
    c.raw(
        f'<line x1="{origin[0]:.1f}" y1="{origin[1]:.1f}" x2="{origin[0]:.1f}" y2="{origin[1]:.1f}" '
        f'stroke="{c.t["text2"]}" stroke-width="1.4" stroke-dasharray="4 4" opacity="0.7">'
        + discrete("x2", [f"{g[0]:.1f}" for g in ghost])
        + discrete("y2", [f"{g[1]:.1f}" for g in ghost])
        + "</line>"
    )
    c.raw(
        f'<line x1="{origin[0]:.1f}" y1="{origin[1]:.1f}" x2="{screen[0][0]:.1f}" y2="{screen[0][1]:.1f}" '
        f'stroke="{colors[0]}" stroke-width="3" stroke-linecap="round">'
        + moving("x2", [s[0] for s in screen])
        + moving("y2", [s[1] for s in screen])
        + discrete("stroke", colors)
        + "</line>"
    )
    c.raw(
        f'<circle cx="{screen[0][0]:.1f}" cy="{screen[0][1]:.1f}" r="6" fill="{colors[0]}" '
        f'stroke="{c.t["surface"]}" stroke-width="2">'
        + moving("cx", [s[0] for s in screen])
        + moving("cy", [s[1] for s in screen])
        + discrete("fill", colors)
        + "</circle>"
    )

    # Right side: the stage name, its formula, and the three coordinates.
    left = 520
    for k, (_, shown, color, title, formula) in enumerate(stages):
        visible = discrete("opacity", [1 if i == k else 0 for i in range(n)])
        c.raw(f'<g opacity="{1 if k == 0 else 0}">{visible}')
        c.text(left, 70, f"Step {k + 1} of {n}", size=13, color="text2")
        c.text(left, 98, title, size=22, weight=700, color=color if color != "text2" else "text")
        c.text(left, 124, formula, size=14, color="text2")
        for i, value in enumerate(shown):
            c.text(left + 340, 184 + i * 44, f"{value:+.3f}", size=14, weight=600)
        c.raw("</g>")

    bar_scale = 200
    for i in range(3):
        top = 168 + i * 44
        c.text(left, top + 16, f"c{'₁₂₃'[i]}", size=14, color="text2")
        widths = [bar_scale * abs(vector[i]) for vector, *_ in stages]
        c.raw(
            f'<rect x="{left + 30}" y="{top}" width="{widths[0]:.1f}" height="22" rx="4" fill="{colors[0]}">'
            + moving("width", widths)
            + discrete("fill", colors)
            + "</rect>"
        )
    c.text(left, 318, "Bar length = size of each coordinate, at the arrow's scale.", size=12, color="text2")
    c.text(left, 338, "After mixing, the three bars are about equal: that is why one grid fits all.", size=12, color="text2")
    c.text(24, 396, "Dashed arrow = the previous step. Loops every 17 seconds.", size=12, color="text2")
    return c.svg("Animation: one vector moving through each Spherra transform")


def main():
    ex = example()
    OUT.mkdir(parents=True, exist_ok=True)
    for theme in THEMES:
        (OUT / f"transform-animation-{theme}.svg").write_text(animation_figure(theme, ex))
        (OUT / f"compression-{theme}.svg").write_text(compression_figure(theme, ex))
        (OUT / f"search-{theme}.svg").write_text(search_figure(theme, ex))
    for key in ("u", "y", "p", "e", "e_hat", "r", "q"):
        print(f"{key:>9} = {fmt(ex[key], 3)}")
    print(f"    codes = {ex['codes']}  centroid = {ex['centroid']}")
    print(f"   |e|    = {norm(ex['e']):.3f}   |y-r| = {norm(sub(ex['y'], ex['r'])):.3f}   |r| = {norm(ex['r']):.3f}")
    for key in ("truth", "truth_rotated", "primary", "refined", "corrected"):
        print(f"{key:>13} = {ex[key]:.4f}")


if __name__ == "__main__":
    main()
