"""Generate cqa-grader's brand/ files: mark, lockups, subset fonts.

The two inputs come from the google/fonts repository:
`ofl/cabin/Cabin[wdth,wght].ttf` and `ofl/cabinsketch/CabinSketch-Bold.ttf`.
Run from the repo root (bump brand/VERSION if any output changes):

    uv run --no-project --with fonttools==4.55.3 --with brotli==1.1.0 \\
        --with uharfbuzz==0.45.0 python scripts/gen-brand.py \\
        Cabin[wdth,wght].ttf CabinSketch-Bold.ttf brand
"""

import math
import shutil
import sys
import tempfile
from pathlib import Path

import uharfbuzz as hb
from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.subset import Options, Subsetter
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

cabin_var, sketch_path, out = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
(out / "fonts").mkdir(parents=True, exist_ok=True)
work = Path(tempfile.mkdtemp())  # instanced fonts; outside the repo if a step fails

LATIN = list(range(0x20, 0x7F)) + list(range(0xA0, 0x100)) + [
    0x2013, 0x2014, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2026, 0x2192, 0x2265, 0x2286, 0x2287,
]


def f(x):
    s = f"{x:.2f}".rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


# --- fonts ----------------------------------------------------------------

def subset_woff2(font, dest, unicodes=LATIN, rename=None):
    opts = Options()
    opts.flavor = "woff2"
    opts.layout_features = ["*"]
    opts.name_IDs = ["*"]
    opts.notdef_outline = True
    s = Subsetter(opts)
    s.populate(unicodes=unicodes)
    s.subset(font)
    if rename:
        # A subset is a Modified Version under the OFL, and may not carry a
        # Reserved Font Name: replace every name record that would.
        table = font["name"]
        for rec in list(table.names):
            if rec.nameID in (1, 3, 4, 6, 16, 17, 18, 21, 22, 25):
                table.removeNames(nameID=rec.nameID)
        style = "Bold"
        table.setName(rename, 1, 3, 1, 0x409)
        table.setName(style, 2, 3, 1, 0x409)
        table.setName(f"{rename} {style}", 4, 3, 1, 0x409)
        table.setName(f"{rename.replace(' ', '')}-{style}", 6, 3, 1, 0x409)
        table.setName(f"{rename} {style}; cqa-grader", 3, 3, 1, 0x409)
    # Reproducible output: keep the source's dates, not the time of this run.
    font.recalcTimestamp = False
    font["head"].modified = font["head"].created
    font.save(dest)


# Cabin: keep weight variable (400-700) at normal width, for the site's
# body text and the wordmark's "grader".
cabin_path = work / "cabin-normal-width.ttf"
instantiateVariableFont(TTFont(cabin_var), {"wdth": 100, "wght": (400, 700)}).save(cabin_path)
subset_woff2(TTFont(cabin_path), out / "fonts/cabin-latin.woff2")
# Cabin Sketch sets only the wordmark's CQA, and its outlines are heavy.
subset_woff2(
    TTFont(sketch_path),
    out / "fonts/cqa-wordmark-sketch.woff2",
    [ord(c) for c in "CQA"],
    rename="CQA Wordmark Sketch",
)

# --- glyph outlines -------------------------------------------------------

bold_path = work / "cabin-bold.ttf"
instantiateVariableFont(TTFont(cabin_var), {"wdth": 100, "wght": 700}).save(bold_path)


def shape(font_path, text, x0):
    """Outline `text` at 1000 units per em, baseline at y=0, y down."""
    blob = hb.Blob.from_file_path(str(font_path))
    face = hb.Face(blob)
    font = hb.Font(face)
    buf = hb.Buffer()
    buf.add_str(text)
    buf.guess_segment_properties()
    hb.shape(font, buf, {"kern": True, "liga": False})
    tt = TTFont(font_path)
    upm = tt["head"].unitsPerEm
    k = 1000 / upm
    glyphs = tt.getGlyphSet()
    order = tt.getGlyphOrder()
    pen = SVGPathPen(glyphs, ntos=f)
    bounds = BoundsPen(glyphs)
    x = 0
    for info, pos in zip(buf.glyph_infos, buf.glyph_positions):
        name = order[info.codepoint]
        t = (k, 0, 0, -k, x0 + (x + pos.x_offset) * k, -pos.y_offset * k)
        glyphs[name].draw(TransformPen(pen, t))
        glyphs[name].draw(TransformPen(bounds, t))
        x += pos.x_advance
    cap = getattr(tt["OS/2"], "sCapHeight", 700) * k
    return pen.getCommands(), bounds.bounds, x0 + x * k, cap


# --- the mark, D6 ----------------------------------------------------------
# 24x24 units. Open rings (r 2.2, stroke 1.8) at A and B, a filled anchor
# (r 2.9) at C, and two butt-capped edges 1.8 wide that end on the rings'
# centrelines. As fills: every outer contour clockwise, every hole
# counter-clockwise, so the nonzero rule unions the overlaps.

A, B, C = (3.5, 11.5), (9.5, 17.5), (20.5, 5.5)
R, W, ANCHOR = 2.2, 1.8, 2.9


def circle(c, r, clockwise=True):
    (x, y), s = c, 1 if clockwise else 0
    return f"M{f(x - r)} {f(y)}A{f(r)} {f(r)} 0 1 {s} {f(x + r)} {f(y)}A{f(r)} {f(r)} 0 1 {s} {f(x - r)} {f(y)}Z"


def edge(p, q):
    dx, dy = q[0] - p[0], q[1] - p[1]
    n = math.hypot(dx, dy)
    ux, uy = dx / n, dy / n
    a = (p[0] + ux * R, p[1] + uy * R)
    b = (q[0] - ux * R, q[1] - uy * R)
    nx, ny = -uy * W / 2, ux * W / 2
    pts = [(a[0] + nx, a[1] + ny), (b[0] + nx, b[1] + ny), (b[0] - nx, b[1] - ny), (a[0] - nx, a[1] - ny)]
    area = sum(x1 * y2 - x2 * y1 for (x1, y1), (x2, y2) in zip(pts, pts[1:] + pts[:1]))
    if area < 0:  # y down: positive is clockwise
        pts.reverse()
    return "M" + "L".join(f"{f(x)} {f(y)}" for x, y in pts) + "Z"


mark = "".join([
    circle(A, R + W / 2), circle(A, R - W / 2, clockwise=False),
    circle(B, R + W / 2), circle(B, R - W / 2, clockwise=False),
    circle(C, ANCHOR),
    edge(A, B), edge(B, C),
])
(out / "mark.path").write_text(mark + "\n")
(out / "mark.svg").write_text(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" role="img" aria-label="cqa-grader">'
    f'<path fill="#c0392b" d="{mark}"/></svg>\n'
)

# --- lockups -----------------------------------------------------------------
# The mark is 1em square, then 0.3em of space, then CQA and grader with no
# space between, all at 1000 units per em. The mark is centred on the
# middle of the capitals, as the site's flex row centres it.

GAP = 300
cqa_d, cqa_b, x_after, cap = shape(sketch_path, "CQA", 1000 + GAP)
grader_d, grader_b, x_end, _ = shape(bold_path, "grader", x_after)
mid = -cap / 2
mark_top = mid - 500
top = min(mark_top, cqa_b[1], grader_b[1]) - 40
bottom = max(mid + 500, cqa_b[3], grader_b[3]) + 40
left, right = -40, max(x_end, cqa_b[2], grader_b[2]) + 40
view = f"{f(left)} {f(top)} {f(right - left)} {f(bottom - top)}"

for name, accent, ink in [("dark", "#f2d16b", "#f3f1e8"), ("light", "#c0392b", "#1f2a44")]:
    (out / f"lockup-{name}.svg").write_text(
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{view}" role="img" aria-label="cqa-grader">'
        f'<path fill="{accent}" transform="translate(0 {f(mark_top)}) scale({f(1000 / 24)})" d="{mark}"/>'
        f'<path fill="{accent}" d="{cqa_d}"/>'
        f'<path fill="{ink}" d="{grader_d}"/></svg>\n'
    )

shutil.rmtree(work)
print(f"cap {cap:.0f}, width {right - left:.0f}, height {bottom - top:.0f}")
