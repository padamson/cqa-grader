# The cqa-grader brand

The mark, the wordmark, the palette and their fonts, in one directory a
host site can copy whole. `VERSION` is bumped whenever anything here
changes; a copy records the version it took. `scripts/gen-brand.py`
regenerates the mark, the lockups and the fonts.

| File | What it is |
|---|---|
| `mark.path` | The mark as SVG path data for a `0 0 24 24` viewBox, drawn in `currentColor`: a check mark whose two strokes are edges in a graph, from two open records to a filled one, the anchor an answer had to reach. |
| `mark.svg` | The same path as a standalone SVG in red pen (`#c0392b`), for places that cannot set a color, such as docs.rs and a favicon. |
| `lockup-light.svg`, `lockup-dark.svg` | The mark beside the wordmark, with the glyphs as outlines, for places that load no web fonts, such as a GitHub README. Light is red pen and ink for a white page; dark is the landing site's chalk on the board. |
| `wordmark.css` | The CQAgrader lockup: the two faces and the accent (`--brand-accent`, chalk-yellow when unset). The comment at its top is the spec. |
| `palette.css` | The board (dark) and paper (light) colors as custom properties. |
| `fonts/` | Cabin (Latin, weights 400 to 700) and the wordmark's sketch face, woff2, under the SIL Open Font License (`OFL-Cabin.txt`, `OFL-CabinSketch.txt`). |

## Using it

- The mark: an inline `<svg viewBox="0 0 24 24" fill="currentColor"><path d="…"/></svg>` with the contents of `mark.path`, so it takes the surrounding text color. It holds its shape at 16 pixels; the rings' holes close up below that.
- The wordmark: import `wordmark.css`, copy `fonts/` beside the built stylesheet, set `--brand-accent` to the host's accent, and use the markup in the comment at its top.
- Pass and fail always carry a tick or a cross as well as a color, so the two never differ by hue alone.

## The sketch face

`cqa-wordmark-sketch.woff2` is Cabin Sketch Bold cut down to the three
letters C, Q and A. Cabin Sketch reserves its font names under the OFL,
and a subset is a modified version, so this one carries its own name,
CQA Wordmark Sketch, and keeps the original copyright and license.
