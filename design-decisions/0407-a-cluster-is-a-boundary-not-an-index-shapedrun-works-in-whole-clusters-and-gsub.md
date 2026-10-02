## §407 — A cluster is a boundary, not an index: `ShapedRun` works in whole clusters, and GSUB LookupType 2 (Multiple Substitution)

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

**Context.** §406 left LookupType 2 out with a reason and a plan: one glyph
becomes several, so the run *grows* and two glyphs come to share a cluster —
which `shape.rs` assumed could not happen. This is that follow-up. It is two
changes, and the order matters: `shape.rs` first, because implementing type 2
against the old invariant would have produced a run whose own queries lied
about it.

**Decision, part one: the glyph↔character map is many-to-many in *both*
directions, and every `ShapedRun` query works in whole clusters.** A ligature
already gave several characters one cluster; type 2 gives several glyphs one
cluster. What a cluster is, therefore, is not an index — glyph *n* is not
character *n* in either direction — but a *boundary*: a caret, a cut and a hit
test may only land where a cluster starts, because every one of them is a byte
offset into the string and the offsets a string admits are its character
boundaries.

The three queries were each wrong in their own way under the new case, and the
fix is the same shape in each: walk *groups* of equal cluster, not glyphs.

- `x_of` tested the *next glyph's* cluster to decide whether it had passed the
  target, so with clusters `[0,0,1]` it counted the first half of a decomposed
  character and answered `x_of(0)` with that glyph's advance instead of zero.
  It now tests the next *group's* cluster.
- `fit_end` walked backwards a glyph at a time, so a budget could cut between
  the `e` and the acute of a decomposed `é` and return an offset naming a
  character that is not there in the cut. It now steps whole clusters.
- `offset_at` tipped at a glyph's midpoint, so a two-glyph cluster had a tipping
  point in its middle that no string offset corresponds to. It now tips at the
  *cluster's* midpoint.

Three private helpers (`group_end`, `group_start`, `span_width`) carry the
grouping so the rule lives in one place rather than three. `fit` needed no
change and says why in a comment: it already returns the *failing glyph's*
cluster, which is a cluster start by construction.

**Decision, part two: type 2 is implemented; the run may grow.** `ccmp`
decomposes precomposed characters so GPOS mark attachment can place the pieces
— which is the textbook reason the type exists, and is not hypothetical here:
Cambria on the development host decomposes `é` into `e` + acute, and the new
host test `shaped_runs_agree_with_their_strings_about_boundaries` finds exactly
that one grown run in 281 GSUB faces × 6 strings. Every glyph of a sequence
carries the cluster of the glyph it replaced, because they all came from the
same character. Walking resumes *after* the inserted glyphs, so a font that
decomposes A into A-and-something cannot loop.

**An empty `Sequence` is refused, and refused in the subtable rather than the
lookup.** `glyphCount == 0` would delete the glyph; the spec forbids it and
some shapers honour it anyway. Deleting takes the cluster with it, leaving a
character that no caret position corresponds to — worse than a character drawn
as it arrived. The placement is the subtle half: refusing inside `sequence_at`
lets `find_map` try a *later subtable of the same lookup*, whereas the earlier
draft's check in the caller short-circuited the whole lookup. The distinction
was invisible until mutation testing: removing the guard changed nothing,
because the caller's check masked it. Removing the *caller's* check made the
guard load-bearing and the mutation fatal to two named tests.

**`sequence_at` clears the output buffer on entry, not the caller.** Found
while resolving that redundancy, and a genuine bug rather than a tidiness
point: a subtable that reads half a sequence and then finds the table truncated
leaves those glyphs behind, and `find_map` hands the same buffer to the next
subtable — so subtable 1's junk is prepended to subtable 2's answer
(`[44, 30, 31]` where `[30, 31]` was right). The buffer exists to keep ordinary
text from allocating once per position; making it the callee's to clear keeps
that without letting a failed read be seen as part of a successful one.

**Bounds.** `MAX_SEQUENCE = 16`, deliberately set to match `MAX_COMPONENTS` —
they are inverses of each other, and a font that decomposes one glyph into more
than sixteen is malformed rather than ambitious. Exactly the cap is still
allowed: the bound is on absurdity, not on a font sitting at the limit.

**What is still out.** Type 3 (Alternate) and `locl`, both for the reasons in
§406, and types 5/6 (contextual/chaining), which the ordered lookup list now
makes implementable and which `clig`/`calt` need.

**Where it lives.** `gsub::{apply_multiple, sequence_at, MAX_SEQUENCE}` in
`gui/font/src/gsub.rs`; `ShapedRun::{group_end, group_start, span_width, x_of,
fit_end, offset_at}` and the `ShapedGlyph::cluster` doc in
`gui/font/src/shape.rs`; the host tests `installed_fonts_leave_a_tab_alone` and
`shaped_runs_agree_with_their_strings_about_boundaries` in
`gui/font/tests/host_fonts.rs`.
