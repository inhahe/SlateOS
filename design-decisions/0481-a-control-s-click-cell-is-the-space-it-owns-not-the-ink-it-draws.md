## §481 — A control's click cell is the space it owns, not the ink it draws

**Date:** 2026-08-20
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the emoji picker draws six skin-tone swatches as 18-pixel circles
24 pixels apart, and only the circles could be clicked — so a fifth of the strip
did nothing. Fixing it meant deciding what a swatch's clickable area *is*, and
then working out how to test a layout after collapsing its two copies into one
function, which removes the very disagreement most of these tests look for.

### The click cell is the pitch, not the circle

Two defensible answers:

| | Clickable area | Consequence |
|---|---|---|
| The drawn circle | 18px of every 24px | the gaps are dead; aiming between two tones does nothing |
| The whole pitch | 24px of every 24px | the gaps go to the nearer circle; a click always means something |

Chosen: **the pitch**. The argument is that the gap is not a region the user
perceives as "not a swatch" — it is the space *between* swatches, six pixels
wide, well under the accuracy anyone aims with. A user who lands there was
aiming at a swatch, and the nearest one is the only reasonable reading of their
intent. There is no competing target to steal from: the gaps are interior to a
strip that holds nothing else.

The counter-argument — that a control should be exactly as big as it looks — is
real and is why this is a decision rather than an obvious fix. It is right for
controls that *abut* other controls, where growing one shrinks its neighbour.
It is wrong here, where the alternative owner of those pixels is nothing at all.

The same reasoning settles the smaller call alongside it: **a click on the
strip's label is consumed, not ignored.** The strip is part of the popup;
"ignored" means the click is offered to whatever is behind, and a user who
misses a swatch by a few pixels should not thereby activate a window underneath.

**`tab_at` rejects negative `x` explicitly** rather than relying on the cast. A
float-to-integer cast in Rust *saturates*, so `-30.0 as usize` is `0` — a click
off the left edge of the window was selecting tab 0. The guard carries a comment
naming the saturation, because without it the guard reads as dead code that a
later reader will delete.

### The testing problem this creates — §479's rule from the other side

§475–§479 are five instances of one fault: a renderer and a hit test each
spelling out the same arithmetic, and drifting. The fix each time is to collapse
them into one named function. **But the collapse removes the disagreement the
tests were looking for.** Once paint and band both read `grid_left()`, a test
that clicks where the grid was painted passes for *any* `grid_left`, correct or
not — and that is not hypothetical: mutating the centring divisor from `2.0` to
`4.0` survived the entire first mutation run.

So the collapse must be paid for with a check of a different kind:

**Measure the painted geometry against something the layout function does not
get to decide.** Not "the columns start where `grid_left()` says" — that is a
tautology — but "the painted columns leave equal margins on the left and right
of `WINDOW_WIDTH`". `grid_left` has no vote on that. Likewise the grid's clip
rectangle is checked against the *painted skin-tone strip*, not against
`grid_bottom()`: two renderers agreeing is evidence; a renderer agreeing with
the function it called is not.

Stated as a rule: **after collapsing duplicated geometry, every layout function
needs one assertion whose expected value comes from outside it.** The window's
own dimensions and a second, independently-painted element are the two sources
available. §479 arrived at the same requirement from a different direction —
there the concern was where an expectation comes from, here it is what a
collapse costs — which is a reasonable sign the rule is load-bearing.

The companion mechanical rule, learned from the second escape: **a sweep that
tests "which cell does this x hit" must extend past the last cell.** A sweep
bounded by the controls it knows about cannot see a hit test that wraps, clamps
or saturates beyond them, and all three are ordinary integer-cast behaviours. It
sweeps the whole window now.
