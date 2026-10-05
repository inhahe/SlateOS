## §453 — A performance guard asserts a *ratio* against an in-process control, not a wall-clock figure

**Date:** 2026-08-17
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** text shaping was ~1400x slower than it should be, and after
fixing it we wanted a test that fails automatically if the bug ever comes
back. The obvious form is "shaping 80 characters must take under N
microseconds" — but N has to survive a slow machine, a debug build and a
machine that is busy doing something else, and by the time it is loose enough
for all three it is loose enough to let the bug back in. Instead the test times
a *second* thing in the same process — the built-in bitmap font, which does the
same work minus the part that was slow — and asserts the ratio between them.
A slow machine slows both, so the ratio does not move.

### Context

`C-FONT-SHAPING-IS-1400X-SLOWER-THAN-IT-SHOULD-BE` (`known-issues.md`) was a
per-character constant of ~64 microseconds in `ScaledFont::shape`, caused by
`GSUB` running all 147 of a face's lookups over every glyph in the run. The fix
(coverage digests, HarfBuzz's `hb_set_digest_t`) recovered 19.6x. The entry
itself asked for "a regression assertion on the *measured* rate (something like
'80 characters shape in under 200 us')".

That figure would not have worked. The measurement it came from was a release
build on an idle development host; the same code measures **4,701 us** in a
debug build on the same machine, which is what `cargo test --workspace` runs.
A threshold that admits the debug figure is 23x above the release one, and the
broken code was only 19.6x worse than the fixed code — so the single number
that satisfies both constraints does not exist.

### Decision

`the_layout_tables_do_not_dominate_shaping` in `gui/toolkit/src/text.rs` times
two shapings of the same 80-character string, back to back in one process:

| | what it exercises |
|---|---|
| system face | the whole pipeline, `GSUB` and `GPOS` included |
| built-in bitmap face | the same pipeline with no layout tables at all |

and asserts `system / builtin < 500`. Measured today: **80x in release, 147x
in debug**. The bug sat at **1400–2200x**.

The built-in face is the control. It absorbs everything the absolute threshold
could not: the debug/release factor, the host's speed, and whatever else the
machine is doing during the run — all of which scale both numbers together.
What survives the division is the quantity that actually regressed, which is
what the layout tables charge over the rest of shaping.

### Alternatives considered

**A wall-clock threshold, as the issue proposed.** Rejected above: no single
number is simultaneously loose enough for a debug build and tight enough to
catch a 20x regression.

**Two thresholds, one per profile,** switched on `cfg!(debug_assertions)`.
Workable, but it encodes this host's speed twice instead of once and gets no
less wrong on different hardware. The ratio needs neither.

**Leave it `#[ignore]`d, like the other two timing tests in that module.**
This is what the module did before, and it is exactly how the bug reached
production: the instrument that would have caught it did not exist, and an
instrument that exists but never runs is barely better. A guard nobody runs
guards nothing.

**Benchmark it under `criterion` with a baseline file.** The right shape for
tracking a performance *curve*, and the project already uses it in `bench/`.
Rejected here because the question is binary — has the skip stopped working —
and a criterion baseline is a checked-in file recording one machine's speed,
which is the absolute-threshold problem again with more ceremony.

### Consequences

- The guard runs in the default `cargo test` on every lane, in debug, in
  parallel with 750 other tests, and has ~3.4x margin at its worst measured
  point. It is not a precise measurement and does not try to be; the two
  `#[ignore]`d instruments beside it remain the place to get numbers.
- It is **vacuous on a machine with no system fonts**, where `with_font` falls
  back to the built-in face and the ratio is 1. That is stated in the test
  rather than defended against: a machine with no fonts cannot exhibit the bug.
- The technique generalises to any performance guard where an in-process
  control exists that shares the environment but not the suspect code. Where no
  such control exists, this reasoning does not transfer and a criterion
  baseline probably is the answer.

### Where it lives

`gui/toolkit/src/text.rs` → `mod shaping_cost` →
`the_layout_tables_do_not_dominate_shaping`, alongside the two `#[ignore]`d
instruments (`shaping_cost_by_line_length`, `whole_line_against_visible_window`)
that produced the numbers quoted here.
