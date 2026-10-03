## TD-FONT-CHECKS-FEATURE-MASKS-ONLY-AT-THE-APPLIED-POSITION

**Status:** FIXED 2026-08-14 -- the resolution note in the entry below gives the change; on `main` since. Stamped 2026-09-28 by lane F, `gui/font`'s owner since the six-lane split, so that the heading no longer reads as open.

**What.** The per-glyph feature mask added with the joining shaper is tested
against the lookup's mask at the position the lookup is *applied* to, and
nowhere else. HarfBuzz tests it at every position a lookup matches — each
component of a ligature, each glyph of a context's input sequence.

**Symptom.** A face could write "the final form of this letter, when followed
by that one" as a `fina` ligature; we would form it if the first glyph is
final-form, without checking the second. In practice the second glyph of such
a ligature is always in the same joining state as the first — that is what
makes the ligature meaningful — so this has produced no observed divergence
in the 556-face sweep. It is a narrowness in the model, not a measured bug.

**Why it is filed rather than fixed.** The check wants to live in the same
skipping iterator that `TD-FONT-IGNORES-GSUB-LOOKUP-FLAGS` needs, since both
are "should this matcher consider this glyph". Building two separate
mechanisms and then merging them is more work than waiting.

**Proper fix.** Fold the mask test into the `Skipper` described above, so
every matcher gets it for free.

**Where.** `gui/font/src/gsub.rs` — `apply_lookup`, which is the single place
the mask is consulted today.

**Resolved (2026-08-14).** Folded into `Skipper` as intended: the mask is a
field of the iterator, and `at_or_after` / `prev` return `None` for a glyph
whose mask does not intersect the lookup's, so every matched position is
gated, not just the applied one. One thing the fold had to get right that the
entry above did not anticipate: the gate applies to the *input* only.
`Skipper::context()` returns the same iterator with an all-ones mask, and the
backtrack and lookahead walks use it — a neighbour is a neighbour whatever
feature reached the rule, and gating context on the mask made every chaining
`fina` rule fail when its lookahead was a medial letter.
