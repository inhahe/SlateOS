## TD-C-THE-COMPOSE-PATH-HAS-NO-TIMING-GUARD-IN-THE-DEFAULT-RUN -- FIXED 2026-09-13

**Date:** 2026-09-13. **Lane:** C.
**Where:** `gui/compositor/src/lib.rs` —
`the_demo_scene_composites_inside_the_frame_ceiling`, now `#[ignore]`d.

**In short:** there used to be a test that failed if drawing one frame got ten
times slower, and it ran every time anyone ran the tests. It has been switched
off by default, because it was also failing when the computer was merely busy,
and it had no way to tell the two apart. Nothing is broken today; what is gone
is an alarm that would have gone off the next time somebody made drawing slow.

**Why it had to go.** Three firings in two days, with these figures:

| frame | what it was |
|---|---|
| 67_074 us | a real regression — `contrast_ratio` doing three `powf(2.4)` per colour, once per blurred window per frame |
| 54_434 us | contention; the same test alone took ~5_000 us |
| 53_363 us | contention |

The real one and the noise **overlap**, so no ceiling separates them. Under a
saturated machine a wall-clock bound cannot tell the thing it watches for from
the conditions it watches under. Best-of-five was the previous attempt at this
and does not help: five consecutive samples during a workspace run are five
contended samples.

**What the default run still has:** that the scene composites, that the window
count is right, and that the frame took a non-zero time — i.e. that it did
work at all.

**The proper fix is to count work rather than measure time.** A guard immune
to load asserts an *operation count*: how many times the palette is resolved
per frame, how many pixels the compose path writes for a given damage set.
`tests/damage_narrows_the_work.rs` already does the ratio form of this and
does not flake. The regression that was caught above would have been caught by
"the palette is resolved at most once per frame", which is a design property,
exactly checkable, and free.

**Second-best fix:** run the `#[ignore]`d benchmarks on a quiet machine as a
scheduled step rather than as part of the correctness run, and diff against
`bench/baselines.toml` the way `scripts/bench-history.py` does for the kernel.

---

**FIXED 2026-09-13 by the first of those — the default run now carries an
operation count, `compositor::tests::composing_a_frame_evaluates_no_transfer_
function`.** It asserts that composing a frame evaluates the sRGB transfer
function **zero** times.

**Why that is the right quantity, rather than the palette-resolve count this
entry proposed.** Resolving a palette per blurred window per frame is a
design choice that may legitimately change, so pinning its count would make a
future refactor look like a regression. What may *not* change is a resolve
being expensive — and the specific regression was `contrast_ratio` doing three
`powf(2.4)` per colour inside the render path. `guitk::theme` precomputes all
256 channel values once per process, so the correct number of evaluations
during a frame is not "few", it is zero. Zero has no noise floor, which is
exactly what the wall-clock ceiling lacked.

**The counter is compiled into every build, not behind `#[cfg(test)]`.** A
`cfg(test)` counter in `guitk` would see only `guitk`'s own unit tests: the
compositor, the desktop and the widgets all link the *non-test* build, so the
latch would sit at zero while the code it guards ran hot — a check reporting
success over a population it cannot see, which is the same defect this entry
is about. The cost is 256 relaxed atomic increments per process, once, because
the instrumented path is cold by construction. Instrumenting the cold path is
free precisely when the regression you fear is "this path became hot".

**Proved able to fire before being believed.** Reverting `relative_luminance`
to recompute the curve per call — the original regression's shape — makes the
new test report **9216** evaluations for one frame of a three-command scene
(36 luminance calls x 256 channel values) and fail loudly. Three orders of
magnitude from the asserted zero, on any machine in any mood. The `#[ignore]`d
wall-clock test stays where it is, for the frame-time figure it still gives
when run deliberately.
