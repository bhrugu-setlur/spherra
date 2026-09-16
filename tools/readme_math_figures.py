#!/usr/bin/env python3
"""Draw the README's worked 4D example of Spherra's compression and scoring math.

The real index works in 768 dimensions. This script runs the same steps on a
4D toy vector, the smallest size with a real Hadamard matrix: normalize, flip
signs, shuffle, fast Walsh-Hadamard transform, round each coordinate to a grid,
and correct the rounding with product-quantization codebooks. The toy grid has
4 levels instead of 16, and each codebook has 4 entries instead of 256, so the
errors are large enough to see. The toy does one round; Spherra does two.

Run with no arguments; it rewrites the SVGs in docs/images and prints the
numbers quoted in the README. Standard library only.
"""

from __future__ import annotations

import math
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "docs" / "images"

# ---- The worked example ---------------------------------------------------

X = (4.0, 2.0, 2.0, 1.0)  # length exactly 5, with one dominant coordinate
QUERY = (2.0, 1.0, 0.0, 2.0)  # length exactly 3
SIGNS = (1.0, -1.0, 1.0, 1.0)
PERMUTATION = (2, 0, 1, 3)  # new coordinate i reads old coordinate PERMUTATION[i]
LEVELS = (-0.6, -0.2, 0.2, 0.6)  # the toy 2-bit grid for every coordinate
PIECES = ((0, 2), (2, 4))  # product quantization: two pieces of two coordinates
CODEBOOKS = (
    ((0.05, 0.05), (-0.15, 0.05), (0.05, -0.15), (-0.1, -0.1)),
    ((0.15, -0.05), (-0.05, 0.15), (0.0, 0.0), (-0.1, -0.1)),
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


def fwht_passes(values):
    """Every intermediate state of the unnormalized fast Walsh-Hadamard transform.

    Mirrors crates/spherra-simd/src/scalar.rs: for half-width h = 1, 2, ...,
    each pair (a, a + h) becomes (left + right, left - right).
    """
    v = list(values)
    states = []
    half = 1
    while half < len(v):
        for start in range(0, len(v), 2 * half):
            for offset in range(half):
                left, right = v[start + offset], v[start + offset + half]
                v[start + offset], v[start + offset + half] = left + right, left - right
        states.append(tuple(v))
        half *= 2
    return states


def rotate(v):
    """One round: signs, shuffle, then FWHT scaled by 1/sqrt(n)."""
    flipped = tuple(c * s for c, s in zip(v, SIGNS))
    shuffled = tuple(flipped[source] for source in PERMUTATION)
    passes = fwht_passes(shuffled)
    y = scale(passes[-1], 1.0 / math.sqrt(len(v)))
    return flipped, shuffled, passes, y


def example():
    u = scale(X, 1.0 / norm(X))
    flipped, shuffled, passes, y = rotate(u)
    codes = tuple(min(range(len(LEVELS)), key=lambda k: abs(c - LEVELS[k])) for c in y)
    p = tuple(LEVELS[code] for code in codes)
    e = sub(y, p)
    entries = []
    e_hat = []
    for (start, end), book in zip(PIECES, CODEBOOKS):
        piece = e[start:end]
        best = min(range(len(book)), key=lambda k: norm(sub(piece, book[k])))
        entries.append(best)
        e_hat.extend(book[best])
    e_hat = tuple(e_hat)
    r = add(p, e_hat)
    z = scale(QUERY, 1.0 / norm(QUERY))
    q = rotate(z)[3]
    return {
        "u": u,
        "flipped": flipped,
        "shuffled": shuffled,
        "passes": passes,
        "y": y,
        "unsigned": scale(fwht_passes(u)[-1], 0.5),
        "codes": codes,
        "p": p,
        "e": e,
        "entries": tuple(entries),
        "e_hat": e_hat,
        "r": r,
        "q": q,
        "truth": dot(z, u),
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
SUB = "₁₂₃₄"


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

    def text(self, x, y, body, size=13, color="text", weight=400, anchor="start", italic=False, extra=""):
        style = ' font-style="italic"' if italic else ""
        self.raw(
            f'<text x="{x:.1f}" y="{y:.1f}" font-size="{size}" font-weight="{weight}" '
            f'fill="{self.t[color]}" text-anchor="{anchor}"{style}>{body}{extra}</text>'
        )

    def line(self, a, b, color="grid", width=1.0, dash=None):
        dashed = f' stroke-dasharray="{dash}"' if dash else ""
        self.raw(
            f'<line x1="{a[0]:.1f}" y1="{a[1]:.1f}" x2="{b[0]:.1f}" y2="{b[1]:.1f}" '
            f'stroke="{self.t[color]}" stroke-width="{width}" stroke-linecap="round"{dashed}/>'
        )

    def svg(self, title):
        return (
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{self.width}" height="{self.height}" '
            f'viewBox="0 0 {self.width} {self.height}" font-family="{FONT}" role="img">'
            f"<title>{title}</title>"
            f'<rect width="100%" height="100%" rx="12" fill="{self.t["surface"]}"/>'
            + "".join(self.parts)
            + "</svg>\n"
        )


def signed_bar(zero, unit, value):
    """Left edge and width of a bar growing right for positive, left for negative."""
    return zero + min(0.0, value) * unit, abs(value) * unit


def bar_chart(c, left, top, values, color, unit, title, outline=None, row_gap=34):
    """A small signed bar chart with one row per coordinate."""
    c.text(left, top, title, size=14, weight=700)
    zero = left + 140
    c.line((zero, top + 14), (zero, top + 14 + row_gap * len(values)), color="grid", width=1.2)
    for i, value in enumerate(values):
        y = top + 24 + i * row_gap
        c.text(left, y + 15, f"c{SUB[i]}", size=12, color="text2")
        c.text(left + 64, y + 15, f"{value:+.2f}", size=12, anchor="end")
        x, width = signed_bar(zero, unit, value)
        c.raw(f'<rect x="{x:.1f}" y="{y}" width="{width:.1f}" height="20" rx="4" fill="{c.t[color]}"/>')
        if outline is not None:
            ox, ow = signed_bar(zero, unit, outline[i])
            c.raw(
                f'<rect x="{ox:.1f}" y="{y - 2}" width="{ow:.1f}" height="24" rx="4" fill="none" '
                f'stroke="{c.t["text"]}" stroke-width="1.2" stroke-dasharray="3 2"/>'
            )


def compression_figure(theme, ex):
    c = Canvas(960, 250, theme)
    unit = 95
    panels = [\
        (ex["u"], "blue", "u: direction", None, "largest 0.80, smallest 0.20"),
        (ex["y"], "blue", "y: after one round", None, "sizes 0.1 to 0.7, length 1"),
        (ex["p"], "orange", "p: rounded to grid", ex["y"], f"miss ‖y − p‖ = {norm(ex['e']):.3f}"),
        (ex["r"], "aqua", "r: p + correction", ex["y"], f"miss ‖y − r‖ = {norm(sub(ex['y'], ex['r'])):.3f}"),
    ]
    for k, (values, color, title, outline, note) in enumerate(panels):
        left = 24 + k * 234
        bar_chart(c, left, 36, values, color, unit, title, outline)
        c.text(left, 212, note, size=12, color="text2")
    c.text(24, 238, "Dashed outline = y, the vector we are trying to store.", size=12, color="text2")
    return c.svg("The example vector at each stage of compression")


def fwht_figure(theme, ex):
    """A butterfly diagram of the two FWHT passes on the shuffled vector."""
    c = Canvas(960, 400, theme)
    columns = [
        ("shuffled input", ex["shuffled"]),
        ("pass 1  (h = 1)", ex["passes"][0]),
        ("pass 2  (h = 2)", ex["passes"][1]),
        ("× 1/√4  =  y", ex["y"]),
    ]
    xs = [70, 330, 590, 820]
    box_w, box_h = 86, 34
    row_y = [110 + i * 66 for i in range(4)]
    c.text(24, 34, "Fast Walsh–Hadamard transform on 4 coordinates", size=16, weight=700)
    c.text(24, 56, "Each pass turns a pair (a, b) into (a + b, a − b). Solid line = add, dashed line = subtract.", size=13, color="text2")

    def node_left(col, row):
        return (xs[col], row_y[row] + box_h / 2)

    def node_right(col, row):
        return (xs[col] + box_w, row_y[row] + box_h / 2)

    for col, half in ((0, 1), (1, 2)):
        for start in range(0, 4, 2 * half):
            for offset in range(half):
                a, b = start + offset, start + offset + half
                c.line(node_right(col, a), node_left(col + 1, a), color="text2", width=1.4)
                c.line(node_right(col, b), node_left(col + 1, a), color="text2", width=1.4)
                c.line(node_right(col, a), node_left(col + 1, b), color="text2", width=1.4)
                c.line(node_right(col, b), node_left(col + 1, b), color="text2", width=1.4, dash="5 4")
    for row in range(4):
        c.line(node_right(2, row), node_left(3, row), color="text2", width=1.4)
    for col, (title, values) in enumerate(columns):
        c.text(xs[col] + box_w / 2, 94, title, size=13, weight=600, anchor="middle")
        for row, value in enumerate(values):
            color = "blue" if col in (0, 3) else "text2"
            c.raw(
                f'<rect x="{xs[col]}" y="{row_y[row]}" width="{box_w}" height="{box_h}" rx="6" '
                f'fill="{c.t["surface"]}" stroke="{c.t[color]}" stroke-width="2"/>'
            )
            c.text(xs[col] + box_w / 2, row_y[row] + 22, f"{value:+.1f}", size=14, weight=600, anchor="middle")
    for row in range(4):
        c.text(xs[0] - 12, row_y[row] + 22, f"c{SUB[row]}", size=13, color="text2", anchor="end")
    return c.svg("Butterfly diagram of the fast Walsh-Hadamard transform")


def hadamard_figure(theme):
    """Sylvester's construction: doubling rule and resulting H1, H2, H4."""
    c = Canvas(960, 290, theme)
    surf = c.t["surface"]

    c.text(24, 34, "Hadamard matrices: Sylvester’s construction", size=16, weight=700)
    c.text(24, 56, "Starts from H₁ = [+1] and doubles size: H₂ₙ combines two copies of Hₙ and flips the lower-right sign.", size=13, color="text2")

    def draw_bracket(x, y, h, is_left=True, color="text2", width=1.6, tick=6):
        if is_left:
            c.raw(
                f'<path d="M{x + tick:.1f},{y:.1f} H{x:.1f} V{y + h:.1f} H{x + tick:.1f}" '
                f'fill="none" stroke="{c.t[color]}" stroke-width="{width}" stroke-linecap="round" stroke-linejoin="round"/>'
            )
        else:
            c.raw(
                f'<path d="M{x - tick:.1f},{y:.1f} H{x:.1f} V{y + h:.1f} H{x - tick:.1f}" '
                f'fill="none" stroke="{c.t[color]}" stroke-width="{width}" stroke-linecap="round" stroke-linejoin="round"/>'
            )

    y_mid = 180
    title_y = 76

    # Panel 1: Sylvester's rule
    x_rule = 45
    c.text(x_rule, y_mid + 6, "H₂ₙ =", size=16, weight=600)

    bx = x_rule + 62
    bw, bh = 50, 38
    gap = 6
    by = y_mid - bh - gap // 2
    c.text(bx + bw + gap / 2, title_y, "Sylvester’s rule", size=13, weight=600, color="text2", anchor="middle")
    draw_bracket(bx - 8, by - 6, bh * 2 + gap + 12, is_left=True)
    draw_bracket(bx + bw * 2 + gap + 8, by - 6, bh * 2 + gap + 12, is_left=False)

    blocks = [
        (0, 0, "Hₙ", "blue"),
        (1, 0, "Hₙ", "blue"),
        (0, 1, "Hₙ", "blue"),
        (1, 1, "−Hₙ", "orange"),
    ]
    for col, row, label, col_color in blocks:
        px = bx + col * (bw + gap)
        py = by + row * (bh + gap)
        c.raw(f'<rect x="{px}" y="{py}" width="{bw}" height="{bh}" rx="6" fill="{surf}" stroke="{c.t[col_color]}" stroke-width="1.8"/>')
        c.text(px + bw / 2, py + bh / 2 + 5, label, size=15, weight=600, anchor="middle", color=col_color)

    # Panel 2: H1
    x_h1 = 300
    c.text(x_h1, y_mid + 6, "H₁ =", size=16, weight=600)
    h1_x = x_h1 + 52
    h1_w, h1_h = 36, 36
    c.text(h1_x + h1_w / 2, title_y, "H₁ (1×1)", size=13, weight=600, color="text2", anchor="middle")
    draw_bracket(h1_x - 8, y_mid - h1_h // 2 - 6, h1_h + 12, is_left=True)
    draw_bracket(h1_x + h1_w + 8, y_mid - h1_h // 2 - 6, h1_h + 12, is_left=False)
    c.raw(f'<rect x="{h1_x}" y="{y_mid - h1_h // 2}" width="{h1_w}" height="{h1_h}" rx="6" fill="{surf}" stroke="{c.t["blue"]}" stroke-width="1.8"/>')
    c.text(h1_x + h1_w / 2, y_mid + 5, "+1", size=13, weight=600, anchor="middle", color="blue")

    # Panel 3: H2
    x_h2 = 460
    c.text(x_h2, y_mid + 6, "H₂ =", size=16, weight=600)
    h2_x = x_h2 + 52
    cw, ch = 34, 34
    h2_gap = 4
    h2_y = y_mid - ch - h2_gap // 2
    c.text(h2_x + cw + h2_gap / 2, title_y, "H₂ (2×2)", size=13, weight=600, color="text2", anchor="middle")
    draw_bracket(h2_x - 8, h2_y - 6, ch * 2 + h2_gap + 12, is_left=True)
    draw_bracket(h2_x + cw * 2 + h2_gap + 8, h2_y - 6, ch * 2 + h2_gap + 12, is_left=False)
    h2_vals = [[1, 1], [1, -1]]
    for r in range(2):
        for col in range(2):
            val = h2_vals[r][col]
            v_color = "blue" if val > 0 else "orange"
            v_str = "+1" if val > 0 else "−1"
            px = h2_x + col * (cw + h2_gap)
            py = h2_y + r * (ch + h2_gap)
            c.raw(f'<rect x="{px}" y="{py}" width="{cw}" height="{ch}" rx="6" fill="{surf}" stroke="{c.t[v_color]}" stroke-width="1.6"/>')
            c.text(px + cw / 2, py + ch / 2 + 5, v_str, size=13, weight=600, anchor="middle", color=v_color)

    # Panel 4: H4
    x_h4 = 655
    c.text(x_h4, y_mid + 6, "H₄ =", size=16, weight=600)
    h4_cw, h4_ch = 30, 30
    h4_gap = 4
    h4_x = x_h4 + 60
    h4_total_w = h4_cw * 4 + h4_gap * 3
    h4_total_h = h4_ch * 4 + h4_gap * 3
    h4_y = y_mid - h4_total_h // 2
    bracket_top = h4_y - 6

    c.text(h4_x + h4_total_w / 2, title_y, "H₄ (4×4)", size=13, weight=600, color="text2", anchor="middle")
    draw_bracket(h4_x - 8, bracket_top, h4_total_h + 12, is_left=True)
    draw_bracket(h4_x + h4_total_w + 8, bracket_top, h4_total_h + 12, is_left=False)

    # Subtle quadrant boundary lines
    mid_line_x = h4_x + 2 * h4_cw + h4_gap + h4_gap / 2
    mid_line_y = h4_y + 2 * h4_ch + h4_gap + h4_gap / 2
    c.line((mid_line_x, h4_y), (mid_line_x, h4_y + h4_total_h), color="grid", width=1.0, dash="2 2")
    c.line((h4_x, mid_line_y), (h4_x + h4_total_w, mid_line_y), color="grid", width=1.0, dash="2 2")

    h4_vals = [
        [1, 1, 1, 1],
        [1, -1, 1, -1],
        [1, 1, -1, -1],
        [1, -1, -1, 1],
    ]
    for r in range(4):
        for col in range(4):
            val = h4_vals[r][col]
            v_color = "blue" if val > 0 else "orange"
            v_str = "+1" if val > 0 else "−1"
            px = h4_x + col * (h4_cw + h4_gap)
            py = h4_y + r * (h4_ch + h4_gap)
            c.raw(f'<rect x="{px}" y="{py}" width="{h4_cw}" height="{h4_ch}" rx="5" fill="{surf}" stroke="{c.t[v_color]}" stroke-width="1.4"/>')
            c.text(px + h4_cw / 2, py + h4_ch / 2 + 5, v_str, size=12, weight=600, anchor="middle", color=v_color)

    return c.svg("Sylvester construction of Hadamard matrices")


def search_figure(theme, ex):
    c = Canvas(960, 300, theme)
    c.text(24, 36, "Scoring the query against the stored vector", size=16, weight=700)
    c.text(24, 58, "q·y is the true cosine similarity. Spherra only has p and r, so it estimates it.", size=13, color="text2")
    rows = [
        ("True cosine", "q·y", ex["truth"], "blue", 1.0),
        ("Quick score", "q·p", ex["primary"], "orange", 1.0),
        ("With correction", "q·r", ex["refined"], "aqua", 0.45),
        ("Length fixed", "q·r / ‖r‖", ex["corrected"], "aqua", 1.0),
    ]
    left = 260
    bar_max = 420
    top_value = max(value for _, _, value, _, _ in rows)
    truth_x = left + bar_max * ex["truth"] / top_value
    for i, (name, formula, value, color, opacity) in enumerate(rows):
        y = 100 + i * 48
        c.text(left - 16, y + 4, name, size=14, weight=600, anchor="end")
        c.text(left - 16, y + 21, formula, size=12, color="text2", anchor="end")
        width = bar_max * value / top_value
        c.raw(
            f'<rect x="{left}" y="{y - 10}" width="{width:.1f}" height="26" rx="4" '
            f'fill="{c.t[color]}" fill-opacity="{opacity}"/>'
        )
        c.text(max(left + width, truth_x) + 10, y + 8, f"{value:.3f}", size=14, weight=600)
    c.line((truth_x, 80), (truth_x, 100 + 3 * 48 + 24), color="text2", width=1.2, dash="4 3")
    c.text(truth_x, 74, "truth", size=12, color="text2", anchor="middle")
    return c.svg("Scores estimated from the compressed vector")


STEP_SECONDS = 2.6  # one stage: a move, then a pause to read it
MOVE_SECONDS = 1.0


def animation_figure(theme, ex):
    """An animated SVG (SMIL) of the four coordinates through every step.

    Each bar keeps its identity through the shuffle, so readers can watch the
    coordinates change rows before the Hadamard passes blend them.
    """
    stages = [
        (scale(X, 0.2), X, "text2", "Start", "x = (4, 2, 2, 1), bars drawn at 1/5 size", None, False),
        (ex["u"], ex["u"], "blue", "1. Normalize", "u = x / ‖x‖ = x / 5, so the length is 1", None, False),
        (ex["flipped"], ex["flipped"], "blue", "2. Flip signs", "multiply by random signs (+1, −1, +1, +1)", None, False),
        (ex["shuffled"], ex["shuffled"], "blue", "3. Shuffle", "new order: old c₃, c₁, c₂, c₄", None, False),
        (ex["passes"][0], ex["passes"][0], "blue", "4. FWHT pass 1 (h = 1)", "pairs (c₁, c₂) and (c₃, c₄): (a, b) becomes (a + b, a − b)", ((0, 1), (2, 3)), False),
        (ex["passes"][1], ex["passes"][1], "blue", "4. FWHT pass 2 (h = 2)", "pairs (c₁, c₃) and (c₂, c₄): (a, b) becomes (a + b, a − b)", ((0, 2), (1, 3)), False),
        (ex["y"], ex["y"], "blue", "4. Scale by 1/√4", "y = pass 2 × ½: length is 1 again, sizes are more even", None, False),
        (ex["p"], ex["p"], "orange", "5. Round to the grid", "p: nearest of −0.6, −0.2, 0.2, 0.6 (dashed lines)", None, True),
        (ex["r"], ex["r"], "aqua", "6. Add the correction", "r = p + ê, picked from two small codebooks", None, True),
    ]
    n = len(stages)
    total = n * STEP_SECONDS
    shuffle_stage = 3
    position = [PERMUTATION.index(k) for k in range(4)]  # row of original coordinate k

    c = Canvas(960, 440, theme)
    zero, unit = 470, 150
    row_top = [150 + i * 62 for i in range(4)]
    bar_h = 30

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

    # Axis, value ticks and the grid levels used in step 5.
    c.line((zero, 136), (zero, row_top[-1] + bar_h + 14), color="grid", width=1.5)
    for tick in (-1.0, 1.0):
        c.line((zero + tick * unit, 136), (zero + tick * unit, row_top[-1] + bar_h + 14), color="grid", dash="2 4")
        c.text(zero + tick * unit, row_top[-1] + bar_h + 32, f"{tick:+.0f}", size=11, color="text2", anchor="middle")
    c.text(zero, row_top[-1] + bar_h + 32, "0", size=11, color="text2", anchor="middle")
    grid_on = discrete("opacity", [1 if stage[6] else 0 for stage in stages])
    levels = "".join(
        f'<line x1="{zero + level * unit:.1f}" y1="136" x2="{zero + level * unit:.1f}" '
        f'y2="{row_top[-1] + bar_h + 14}" stroke="{c.t["orange"]}" stroke-width="1.2" stroke-dasharray="4 3"/>'
        for level in LEVELS
    )
    c.raw(f'<g opacity="0">{grid_on}{levels}</g>')

    # Dashed outline of y while rounding and correcting, to show the miss.
    outline = "".join(
        f'<rect x="{signed_bar(zero, unit, ex["y"][row])[0]:.1f}" y="{row_top[row] - 3}" '
        f'width="{signed_bar(zero, unit, ex["y"][row])[1]:.1f}" height="{bar_h + 6}" rx="5" fill="none" '
        f'stroke="{c.t["text"]}" stroke-width="1.2" stroke-dasharray="3 2"/>'
        for row in range(4)
    )
    c.raw(f'<g opacity="0">{grid_on}{outline}</g>')

    colors = [c.t[stage[2]] for stage in stages]
    for k in range(4):
        rows = [k if s < shuffle_stage else position[k] for s in range(n)]
        values = [stages[s][0][rows[s]] for s in range(n)]
        xs = [signed_bar(zero, unit, v)[0] for v in values]
        widths = [signed_bar(zero, unit, v)[1] for v in values]
        ys = [row_top[row] for row in rows]
        c.raw(
            f'<rect x="{xs[0]:.1f}" y="{ys[0]}" width="{widths[0]:.1f}" height="{bar_h}" rx="4" fill="{colors[0]}">'
            + moving("x", xs)
            + moving("width", widths)
            + moving("y", ys)
            + discrete("fill", colors)
            + "</rect>"
        )

    for row in range(4):
        c.text(zero - 250, row_top[row] + 20, f"c{SUB[row]}", size=15, color="text2")

    # Brackets showing which rows each Hadamard pass pairs up.
    for s, stage in enumerate(stages):
        if stage[5] is None:
            continue
        visible = discrete("opacity", [1 if i == s else 0 for i in range(n)])
        paths = []
        for depth, (a, b) in enumerate(stage[5]):
            x0 = zero - 270 - depth * 18
            ya, yb = row_top[a] + bar_h / 2, row_top[b] + bar_h / 2
            paths.append(
                f'<path d="M{x0 + 14},{ya} H{x0} V{yb} H{x0 + 14}" fill="none" '
                f'stroke="{c.t["blue"]}" stroke-width="2"/>'
            )
        c.raw(f'<g opacity="0">{visible}{"".join(paths)}</g>')

    # Stage titles, formulas and exact values.
    for s, (_, shown, color, title, formula, _, _) in enumerate(stages):
        visible = discrete("opacity", [1 if i == s else 0 for i in range(n)])
        c.raw(f'<g opacity="{1 if s == 0 else 0}">{visible}')
        c.text(24, 44, f"Step {s + 1} of {n}", size=13, color="text2")
        c.text(24, 74, title, size=24, weight=700, color=color if color != "text2" else "text")
        c.text(24, 100, formula, size=15, color="text2")
        for row, value in enumerate(shown):
            c.text(920, row_top[row] + 21, f"{value:+.2f}", size=16, weight=600, anchor="end")
        c.raw("</g>")

    c.text(24, 424, "Each bar is one coordinate. Loops every 23 seconds.", size=12, color="text2")
    return c.svg("Animation: a 4D vector moving through each Spherra transform")


def main():
    ex = example()
    OUT.mkdir(parents=True, exist_ok=True)
    for theme in THEMES:
        (OUT / f"transform-animation-{theme}.svg").write_text(animation_figure(theme, ex))
        (OUT / f"fwht-{theme}.svg").write_text(fwht_figure(theme, ex))
        (OUT / f"hadamard-matrix-{theme}.svg").write_text(hadamard_figure(theme))
        (OUT / f"compression-{theme}.svg").write_text(compression_figure(theme, ex))
        (OUT / f"search-{theme}.svg").write_text(search_figure(theme, ex))
    for key in ("u", "flipped", "shuffled", "y", "unsigned", "p", "e", "e_hat", "r", "q"):
        print(f"{key:>9} = {fmt(ex[key], 3)}")
    print(f"   passes = {[fmt(v, 3) for v in ex['passes']]}")
    print(f"    codes = {ex['codes']}  entries = {ex['entries']}")
    print(f"   |e| = {norm(ex['e']):.3f}   |y-r| = {norm(sub(ex['y'], ex['r'])):.3f}   |r| = {norm(ex['r']):.3f}")
    for (start, end), book in zip(PIECES, CODEBOOKS):
        piece = ex["e"][start:end]
        print(f"   piece {fmt(piece)} distances {[round(norm(sub(piece, entry)), 3) for entry in book]}")
    for key in ("truth", "truth_rotated", "primary", "refined", "corrected"):
        print(f"{key:>13} = {ex[key]:.4f}")


if __name__ == "__main__":
    main()
