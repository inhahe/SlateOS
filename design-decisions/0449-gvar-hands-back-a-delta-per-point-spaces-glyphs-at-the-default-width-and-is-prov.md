## §449 — `gvar` hands back a delta per point, spaces glyphs at the default width, and is proved by a font we wrote ourselves

**Date:** 2026-08-16
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** An adjustable font (§448) stores its *shapes* as a base outline
plus a table of nudges — "at weight 700, move this corner 40 units right." The
table is `gvar`, and reading it is what makes a variable face actually look
bolder rather than merely claim to. Three things had to be settled while writing
it: **what shape the nudges are handed back in** — this runs once per glyph per
frame, on the compositor's text path, and the table also nudges four invisible
points that are not part of the drawn shape at all; **whether the same table
should also decide how far apart to space the glyphs**, since it technically
knows; and **what counts as proof that any of it is right**, given the
surprising discovery that not one font installed on this machine exercises a
third of the code.

### `gvar` returns a delta vector over the *raw points*, and the caller applies it

`Gvar::deltas` takes the glyph's point array plus its contour ends and returns
one `(dx, dy)` per point **plus four more** — the phantom points. It does not
mutate the glyph and it does not know what a path is.

- *What changes:* nothing visible. It is the seam that makes the next two
  callers possible.
- **Why not have it mutate the glyph**, which is what "apply the deltas" sounds
  like: the four phantom deltas are not points of the outline. They are the
  varied side bearings and advances, they belong to no contour, and
  `Face::outline_at` needs the first of them — the left phantom's `dx` — to
  correct where the glyph sits (`out.translate_x(glyf_shift(gid) - phantom)`).
  A mutating signature would have to smuggle that number out some other way,
  and the obvious way — appending four fake points to the outline — puts
  invisible geometry into a drawable path.
- **Why it pays off immediately:** a *composite* glyph varies by moving its
  components, and its variation point array has one entry per component, not
  per contour point. Because `deltas` only wants "an array of points and a list
  of contour ends", the composite path calls the *same function* with a
  synthesized array of component offsets and an empty `ends`, and gets component
  movement for free — including the rule that an empty `ends` means no
  interpolation, so a component the font did not name simply does not move. A
  glyph-mutating design would have needed a second, near-duplicate
  implementation for composites, which is where a divergent bug would live.
- **Why the argument is `&[GlyphPoint]` and not `&[Point]`**, even though
  `gvar` never reads the on-curve flag: `&[Point]` would force every caller to
  build a stripped copy of the array first, which is an allocation per glyph per
  frame on the compositor's text path, paid to hide a field the callee ignores.
- **Why not a path-to-path transform**, which would be the tidiest signature of
  all: the conversion to a path is lossy in exactly the direction that matters.
  `gvar` numbers points by index in the *raw* array — off-curve control points
  included, phantoms past the end — and a path has already dissolved that
  numbering into curve commands. Reconstructing it would mean reconstructing
  the very thing the path threw away.

### An advance that varies comes from `HVAR`, even though `gvar` also knows it

`gvar`'s phantom points encode the varied advance width. We read them — for the
side-bearing correction, which nothing else provides — and then deliberately do
**not** use them to space the glyph. Until the `ItemVariationStore` reader
lands, a varied glyph is drawn at its varied shape and spaced at its *default*
width.

- *What changes:* at the extremes of a weight axis, letters are very slightly
  too close together. Near the default, nothing.
- **Why not just use the phantom**, which is sitting right there and would make
  the symptom disappear today: it would make two tables the source of truth for
  one number, at a moment when *neither* had been checked against a real file
  for that number. Every variable face on this host puts its advance variation
  in `HVAR` and leaves the `gvar` phantoms at zero — so "use the phantom" would
  have shipped as "the advance never varies", dressed up as a feature. A
  visible, documented gap that the next step closes is better than an invisible
  one that the next step has to *discover* and then contradict.
- The gap is recorded at the call site in `ScaledFont::rasterize_glyph` and in
  `known-issues.md` → `TD-FONT-DOES-NOT-READ-VARIATION-STORES`, not left to be
  inferred.

### The interesting half of `gvar` is tested against a font built in the test file

This host has 556 fonts, 7 of them variable, and across ~2,800 glyphs of those
7 **not one** moves a phantom point, varies a composite's component offset, or
has a tuple whose omitted points make interpolation visible. Those three paths —
roughly a third of the module — are unreachable from any real file here.

**Decision: hand-assemble a small variable font inside `sfnt.rs`'s test module
and compute its expected outlines on paper.**

- *What changes:* the module's hardest paths are covered by exact equality
  against arithmetic done by hand, with no tolerance, rather than by a sweep
  that would have reported success while testing nothing.
- The fixture's axis is deliberately arranged so the two tested instances have
  scalars of exactly 1.0 and 0.5 — values a binary float represents exactly —
  so an expectation like "half of 100 units" is `50`, not `50 ± ε`. A tolerance
  would have hidden the class of bug most likely to be present.
- One glyph carries **two** tuples peaking at opposite ends of the axis, which
  is what tests the rule that a tuple scoring zero still occupies its bytes: a
  reader that skips the advance along with the parse then reads the *next*
  tuple from the wrong offset and draws a wrong glyph rather than erroring.
  A one-tuple-per-glyph fixture cannot express that at all.
- **Cost of the choice:** ~150 lines of byte-level table construction that must
  be maintained by hand, and which no external tool validates. Accepted because
  the alternative is not a cheaper test, it is no test.

**The oracle is split in two for the same reason.**
`gui/font/tools/gvar_oracle.py` is written from the specification, but its
`glyf` reader and its point→path conversion cannot be meaningfully independent
of ours. Those are validated by a *separate* comparison taken at the **default
instance**, where every tuple's scalar is provably zero — so a failure in the
varied table means "we applied the deltas wrong" and a failure in the default
table means "we read the outline wrong", rather than one signal for two
unrelated bugs.

**And every comparison carries a "did it move" assertion.** Agreement between
two readers is satisfiable by a reader that ignores `gvar` entirely and an
oracle that agrees with it about nothing, so
`installed_variable_fonts_vary_their_outlines` separately demands that
non-default rows *differ* from the unvaried glyph, and fails if fewer than
three quarters of them do. The lesson generalises past fonts: a differential
test needs a liveness assertion, or the null implementation passes it.
