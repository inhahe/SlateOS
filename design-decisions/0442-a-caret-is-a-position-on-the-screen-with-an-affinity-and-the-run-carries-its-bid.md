## §442 — A caret is a position on the screen with an affinity, and the run carries its bidi levels to answer for one

**Date:** 2026-08-16
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** When a line mixes English and Hebrew, one place in the *text* can
be two places on the *screen*. Put the cursor between the English and the
Hebrew: it can be drawn at the right end of the English, or at the right end of
the Hebrew word — which is somewhere else entirely, because Hebrew is drawn
starting from the right. Both are correct; which one the user meant depends on
which side they arrived from. This adds that "which side" as a value called an
**affinity**, and makes the two caret queries measure across the screen rather
than through the string.

**The caret carries an affinity rather than being answered with one x.**
`offset_at` (pixel → text position) returns a `Hit { offset, affinity }`, and
`x_of` (text position → pixel) takes an `Affinity`.

*The alternative* — return the one x that the text's own direction implies, and
let the caller live with it — is what almost every naive implementation does,
and it is not so much wrong as unanswerable. At a boundary the two positions
are equally correct and the run has no way to prefer one: the information that
decides it (which direction the user was moving, whether they just typed) lives
in the editor, not in the shaping. Answering anyway makes the caret jump for
half of all boundary crossings and gives the editor nothing to fix it with.
ICU (`UBiDi` leading/trailing), DirectWrite (`isTrailingHit`) and Chromium
(`SelectionAffinity`) all model it explicitly for the same reason. The cost is
one enum on two signatures; on left-to-right text — every run this crate builds
today — the two affinities name the same point, so a caller with no opinion
passes `Downstream` and is not wrong.

**The run stores per-glyph bidi levels, not just the L2 permutation.**
`ShapedRun` gained `levels: Vec<Level>` beside `visual: Vec<u32>`, kept
whenever any level is odd even if the permutation is the identity.

*The alternative* — derive direction from the permutation, since a reversed
stretch is exactly a right-to-left one — is appealingly economical and is
wrong on the smallest case there is. Reversing a **one-glyph** run is the
identity permutation, so a single Hebrew letter between two Latin words looks
unreordered while still being read from its right edge: a caret reading the
permutation alone would put itself at the wrong end of it. The permutation
answers "where is this glyph drawn"; a caret also needs "which end of it does
the reader start at", and only the levels say that. The extra vector is empty
for all left-to-right text, which is the case that has to stay free.

**The old logical-order sum survives under a name that says what it is.**
`width_upto(at, end)` is the previous `x_of` body verbatim.

*The alternative* — delete it, since the caret is now correct — would have
removed the number that truncation and ellipsis placement actually want. Those
callers cut the string and measure the piece; "how wide is the prefix" is a
real question with a monotone answer, and it is only wrong when it is called a
caret. Keeping both, named apart, is also what let the regression assertion in
`tests/host_fonts.rs` be moved rather than deleted: "the caret never moves
backwards as it advances through the string" is true of `width_upto` and is
precisely what a correct visual caret must violate, for the length of every
right-to-left run.

**A cluster is one box.** Both queries treat a cluster — a ligature's several
characters, or a decomposed character's several glyphs — as a single
rectangle whose left edge is the leftmost slot any of its glyphs occupies. That
is sound rather than approximate: rule L2 reverses whole level runs, and a
cluster lies inside one, so its glyphs may be drawn in the other order but are
still drawn side by side.

**Where it lives.** `gui/font/src/shape.rs` — `Affinity`, `Hit`,
`ShapedRun::levels`, `reordered`, `slot_of`, `cluster_box`, `leading_edge`,
`trailing_edge`, `offset_at`, `x_of`, `width_upto`;
`gui/font/src/scaled.rs` — the per-glyph levels are now hoisted out of the
reordering branch and handed to the run; `gui/toolkit/src/text.rs` —
`char_index_at`, which drops the affinity because a character *index* is the
same on both sides of a boundary.
