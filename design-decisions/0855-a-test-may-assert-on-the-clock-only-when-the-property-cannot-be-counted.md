## 855. A test may assert on the clock only when the property cannot be counted

**In short:** some tests check that code is fast. They do it by timing it, and
a timed test fails when the machine is busy rather than when the code is
wrong. Two of ours do this and only one of them should. The rule is: if you can
count the work instead of timing it, count it -- and if you genuinely cannot,
set the bound so loose that only a disaster trips it.

**Date:** 2026-09-15. **Lane:** C. **Decided by:** Claude (autonomous).

**What happened.** `gui/compositor`'s
`redrawing_one_window_costs_a_fraction_of_the_whole_desktop` turned the
workspace gate red. It compares two wall-clock durations and wants the partial
frame to be at least three times cheaper. Under `cargo test --workspace`, with
dozens of test binaries running at once, it measured 2.77x. Re-run alone
minutes later, no code change between: 5.4x, pass.

It already took `min()` of three runs, and the comment above the assertion
already said a tight ratio would fail for noise and chose 3x to avoid that.
Neither helped, because the load was sustained for the length of the run and
the noise scales with whatever else the machine was asked to do. **No threshold
would have helped**, which is the part worth generalising: the problem was not
the number.

**Why a flaky gate is worse than no gate.** It teaches its readers to re-run
rather than to read. The first thing I did was go looking for a compositor
regression that did not exist. And on the one occasion such a test is right, it
is indistinguishable from the occasions it was not -- which is the same shape
this tree has been clearing out all week, an observation that confirms and
falsifies identically.

**The rule, in two clauses.**

1. **If the property can be counted, count it.** "Damage tracking narrows the
   work" is a claim about how much work is done. `FrameStats` gained
   `windows_rendered` -- zeroed per frame, incremented in `render_window` --
   and the test now asserts 4 < 19 instead of comparing microseconds. That
   number does not move when the machine is busy. The timing is still measured
   and printed, because it is why anyone cares; it is no longer what decides
   whether the tree is broken.
2. **If it genuinely cannot be counted, the bound must only catch a
   catastrophe.** `gui/appearance/tests/resolve_cost.rs` is the case that
   should stay timed: there is no counter for "how expensive is this
   function", the regression it exists to catch is a `powf` creeping back into
   the contrast path, and that costs 30-100x rather than 50%. Its bound is
   twenty times the measured figure, and its module docs say why in those
   terms. A bound that only catches a catastrophe is the right bound when only
   catastrophes are possible.

**The alternative considered and rejected:** loosening the damage test's ratio
to 2x, or to 1.5x. It would have passed that day and failed on a busier one,
and each loosening buys less signal for the same flakiness. A ratio tight
enough to mean anything is tight enough to be hit by load, so the choice was
never between 3x and 2x -- it was between timing and counting.

**A correction to the diagnosis, added 2026-09-16.** The conclusion above is
sound and the argument that produced it was not, which is worth separating.

"Failed in the workspace gate, passed alone" was read here as *load*. It is
equally the signature of a **rare flake** and of a **test-order dependency**,
and nothing in the observation distinguishes them. Lane B made the mirror of
this mistake the same week: an `oils` test that failed in company and passed
alone was reported by this lane as order interference, and was in fact a
1-in-32770 collision -- `$RANDOM` seeded from the clock, 131064 of the 2^32
seeds giving two equal draws. No neighbouring test was involved.

**Running alone is one trial; a workspace run is hundreds.** Company gives more
*trials*, not more interference, so a rare event looks exactly like a shared-state
bug. The discriminator is cheap and should be run before any theory is
adopted: **run the failing test alone a thousand times.** If it ever fails
alone, order was never the cause.

The counting fix in this entry happens to immunise against all three causes at
once, so a correct decision survived a guessed diagnosis. That will not always
be true, and a future reader reaching for this entry to justify a rewrite
should confirm *which* of the three they have first -- the remedies differ:
counting for load, a pinned seed for a flake, an isolated fixture for order.

**Where this bites next:** anything measuring a frame, a parse, a layout pass
or a search. Before writing `assert!(elapsed < N)`, ask what the elapsed time
is standing in for. It is usually a count of something -- windows re-rendered,
nodes visited, bytes copied, allocations made -- and the count is both more
precise and immune to the machine.

**Not a ban on benchmarks.** `cargo bench` is where a slow machine costs a
number rather than a red gate, and the performance-targets protocol still
applies in full. This is about the test suite, which every lane pays for
before every boot.
