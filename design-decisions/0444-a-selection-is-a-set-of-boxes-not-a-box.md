## §444 — A selection is a set of boxes, not a box

**Date:** 2026-08-16
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** When you drag across text to highlight it, the highlight was
drawn as one rectangle stretching from the first selected character to the
last. That is only right when the text runs one way. In a line that mixes
English and Hebrew, characters the user did *not* select can sit physically
between the ones they did, so a single rectangle paints them too — the
highlight claims you selected words you never touched. The fix is that asking
for a selection's shape now returns a *list* of rectangles, one per stretch
that really is contiguous on screen, and every caller loops.

`ShapedRun::selection_rects(from, to, end) -> Vec<(f32, f32)>` replaces the
two-edge form `x_of(to) - x_of(from)` at every call site
(`guitk::text::selection_boxes`, `pathbar`'s edit-mode highlight, `textview`'s
selection and search-match highlights).

**Why a list rather than a rectangle.** A byte range is contiguous in the
string by construction; it is *not* contiguous on the screen. Rule L2 of the
bidi algorithm reverses each right-to-left run in place, so a logical range
that straddles a direction change lands as two separated boxes, and the gap
between them is occupied by characters outside the range. The two-edge form has
no way to express that — it can only report one interval — so it necessarily
over-paints. The over-painting is not cosmetic: a highlight is the UI's answer
to "what will Copy copy?", and a wrong answer there is a wrong answer about the
user's own data. The shaped run already knows the exact answer, because it
knows the visual order; the only thing missing was a return type able to carry
it.

*The alternative* — return the bounding box of the selected glyphs — is the
same bug with more arithmetic behind it, and is worse for being harder to
recognise as wrong.

**Why `Vec` rather than `impl Iterator`.** Every other geometry query in
`ShapedRun` is lazy, so this one is the odd member and the departure is worth
recording. It cannot be lazy: the caller wants the boxes in drawing order
(left to right), the map from logical index to *selected* must therefore be
complete before the first box can be emitted, and the glyph drawn first may be
the last one logically. A lazy adaptor would have to build the same map inside
its first `next()` — the allocation is not avoided, only hidden, and hiding it
would cost the caller the ability to ask how many boxes there are.

**Why the sweep runs in slot space, not pixel space.** Adjacent boxes are
coalesced into one, and adjacency is decided by comparing *slot indices* —
integers — rather than by comparing a box's right edge to the next box's left
edge. Both quantities are sums of the same advances, but summed in different
orders, and float addition is not associative: the comparison would need a
tolerance, and any tolerance is a number that is too small on a long line and
too large on a short one. Integers need none. (`textview`'s cross-*span*
coalescing does still compare pixels with a 0.01 tolerance, because separate
spans are separately shaped and there is no shared slot space to appeal to.)

**A cluster is always painted whole.** Selecting one half of a ligature
highlights all of it. This falls out rather than being enforced: L2 reverses
whole level runs and a cluster lies entirely inside one, so a cluster always
occupies one contiguous screen box whatever the reordering did. Marking the
cluster through `group_end` is what makes the run-accumulation see it as one
stretch.

**Where it lives.** `gui/font/src/shape.rs` — `selection_rects` and its five
tests (including `selection_boxes_are_ordered_disjoint_and_add_up`, which
checks all 28 sub-ranges of a six-character bidi string);
`gui/toolkit/src/text.rs` — `selection_boxes`/`selection_boxes_in`;
`gui/toolkit/src/pathbar.rs`, `gui/toolkit/src/textview.rs` — the callers.
