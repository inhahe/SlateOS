## §450 — One table owns each varying number: `HVAR` the advance, `MVAR` the line box, and the outline itself the cap-height

**Date:** 2026-08-17
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** an adjustable ("variable") font stores several numbers *twice*.
How wide a letter is at weight 700 can be worked out from the shape table
(`gvar`, which nudges four invisible points that encode the width) **or** read
straight out of a table that exists to answer exactly that question (`HVAR`).
How tall the capital letters are at weight 700 can be measured off the drawn
shape **or** read out of `MVAR`'s `cpht` entry. In both cases the two answers
are *supposed* to agree, and in a real font they nearly do — which is worse
than if they plainly disagreed, because a small mismatch shows up as text that
is drawn at one width and spaced at another, and nobody can tell which half is
wrong. So each number gets exactly one owner, chosen per number, and the other
source is never consulted.

### The advance comes from `HVAR`, never from `gvar`'s phantom points

`Face::advance_at` adds an `HVAR` delta to the `hmtx` advance.
`Face::outline_at` does read `gvar`'s phantom points — it needs the left
side-bearing correction to place the outline — but nothing derives a *width*
from them.

- *What changes:* a bold instance of a proportional face is spaced at its bold
  widths instead of its regular ones. Before this, a varied glyph was drawn
  varied and spaced default: visibly wrong at the extremes of a weight axis,
  invisible near the default.
- **For `HVAR`:** it is the table whose entire purpose is this number, it is
  present on all 7 of this host's variable faces, and it is a direct lookup
  (glyph id → delta) rather than a reconstruction. `gvar`'s phantoms, by
  contrast, arrive only *after* the full delta/interpolation pipeline has run,
  so an advance derived from them inherits every bug in outline handling.
- **Against:** a face could ship `gvar` phantom deltas and no `HVAR`, and would
  then be spaced at default widths. No such face exists here; all 7 leave every
  one of ~2,800 glyphs' phantom deltas at zero and put the variation in `HVAR`.
  If one ever turns up, the fix is to fall back to the phantoms *when `HVAR` is
  absent* — a fallback, still not a second opinion.
- **Cost:** four call sites in `scaled.rs` had to move from `advance` to
  `advance_at`, and missing one would have left a single measurement stale
  against the rest. That is the real hazard of this decision and is why the
  change was made everywhere at once rather than at the rasterizer alone.

### Ascender, descender and line-gap come from `MVAR`; cap-height and x-height stay measured

`Face::metrics_at` applies `MVAR`'s `hasc`/`hdsc`/`hlgp`. It does **not** apply
`xhgt` or `cpht`, even though 4 host faces carry both and the reader could
trivially fetch them. `ScaledFont::derive_metrics` keeps measuring those two off
the varied outline.

- *What changes:* nothing a user sees today, because both routes give nearly
  the same answer. What changes is which one is authoritative when they differ.
- **Why the split is not arbitrary.** The line box (ascender/descender/gap) is
  *declared*, not observable: no glyph's outline tells you how far apart the
  font wants its lines. `MVAR` is the only source, so it is the source.
  Cap-height and x-height are the opposite — they are a *measurement of a
  shape*, and the shape is right there, already varied by `gvar`. Reading the
  tag instead would let a face's declared cap-height drift from where its `H`
  actually ends, and the underline would then miss the letters it belongs to.
- **The consistency argument settles it.** The non-variable path already
  measures cap-height off the outline rather than reading `OS/2`. Reading
  `MVAR` in the variable path would mean the *same face* answered the same
  question two different ways depending on whether an axis had been set — a
  discontinuity at the default instance, which is the one place where nothing
  should change.
- **Against:** a font whose `cpht` is deliberately different from its drawn
  `H` (a designer's override) is ignored. Accepted: an override that contradicts
  the ink is not information this stack can act on coherently, and the
  non-variable path has always ignored it.

### The reader is proved against a spec-derived oracle *and* against fixtures for what no font reaches

`gui/font/tools/varstore_oracle.py` is written from the OpenType specification
rather than transcribed from `varstore.rs`, and the host tests pin its output
for every named instance of every variable face: 7 faces through `HVAR` (5
varying), 4 through `MVAR` (all 4 varying), each with a floor on how many must
*move* so the null implementation cannot pass.

Three branches are unreachable from any installed font and are covered by
synthetic stores in `varstore.rs`'s own test module instead:

- **A null advance-mapping offset means the implicit map** (outer 0, inner =
  glyph id), not "no variation". The only two host faces that take that path
  are the two monospace ones, whose advances do not vary — so misreading it
  produces a *correct* result on every font here.
- **`LONG_WORDS` doubles both column widths.** No host font sets it.
- **A degenerate region scores 1.0**, following HarfBuzz. Measured: 0 of 199
  region axes on this host are degenerate. The oracle deliberately implements
  the plain specification reading here and says so in its docstring, so the two
  can never agree merely by having been copied from each other.
