## TD-FONT-CARETS-ARE-NOT-BIDIRECTIONAL

**Status: FIXED** (2026-08-16, lane C). Implemented as the entry's own "proper
fix" describes, with one addition it did not anticipate: the permutation is not
enough to tell a caret which side of a glyph the reader starts at, because
reversing a *one-glyph* right-to-left run is the identity permutation. So
`ShapedRun` now stores the per-glyph bidi levels alongside `visual`, and both
queries read direction from those. Three parts:

* **`Affinity` and `Hit`** are new public types. `offset_at` returns a `Hit`
  (offset + affinity) instead of a bare offset; `x_of` takes an `Affinity`. At
  a direction boundary the two affinities give two different x's for the same
  byte offset, which is the concept the entry said was missing.
* **Both queries walk the screen.** `x_of` asks for a cluster's *leading* edge
  — its left edge when drawn left to right, its right edge when not — and
  `offset_at` tests each cluster's drawn box and splits it at its own midpoint,
  with the leading/trailing sides swapped for a right-to-left cluster. A
  cluster is one contiguous box whatever the reordering did, because rule L2
  reverses whole level runs and a cluster lies inside one.
* **`width_upto` keeps the old body** — the logical prefix sum — as an
  explicitly-named measurement for truncation and ellipsis placement.

The monotonicity assertion in `tests/host_fonts.rs` ("the caret must never move
backwards as it advances through the string") moved to `width_upto` with it:
that property is exactly what a correct visual caret must *not* have, and the
assertion only ever passed because `x_of` was a prefix width wearing a caret's
name. `x_of` keeps an in-range check, run for both affinities.

Verified: 717 lib tests, the 18 host-font tests and the bidi conformance suite
green; `clippy --all-targets` clean for `osfont` and `guitk`. See
`design-decisions.md` §442.

**What.** `ShapedRun::x_of` and `ShapedRun::offset_at` convert between a byte
offset and an x position by summing advances in *logical* order. That is the
distance **into the text**, not the distance **across the line**, and the two
are the same number only when the run is not reordered.

**Symptom.** Click and caret placement in right-to-left or mixed text. Given
`hello שלום world`, clicking between the `ש` and the `ל` — visually the right
end of the Hebrew word — gets the byte offset of a character near its left
end. Arrow keys land in the wrong place for the same reason. Nothing renders
wrongly: `draw_order()` is correct and the text looks right; it is only the
mapping back from a pixel that is wrong.

**Why it is filed rather than fixed.** It is not a bug in the sum, it is a
missing concept. In bidirectional text one byte offset has *two* legitimate
caret positions — at a direction boundary the caret can be at the visual end
of the left-to-right run or the visual start of the right-to-left one, and
which is correct depends on which direction the user is typing. Every mature
implementation models this explicitly (a "strong" and a "weak" caret, or an
affinity flag on the position). Answering with one x and calling it done just
moves the wrongness somewhere less visible.

**Proper fix.** `x_of` returns the position in the drawn order: walk
`draw_order()` accumulating advances and stop at the glyph whose cluster is
the offset asked for. `offset_at` does the inverse, and both grow an affinity
so a boundary can be asked about from either side. The existing behaviour
stays available as a measurement — `width_upto`, which is what the truncation
code actually wants and what it uses `x_of` for today.

**Where.** `gui/font/src/shape.rs` — `x_of` and `offset_at`, whose doc
comments both name this entry; `ShapedRun::visual`, which holds everything the
fix needs.
