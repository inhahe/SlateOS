## TD-C-THE-COMPOSITOR-FRAME-BUDGET-HAS-NO-INSTRUMENT -- FIXED 2026-09-13; the first half caught a 30x regression the day it landed


**Correction, 2026-09-13 (later).** This entry's half one -- "no benchmark
series" -- was **wrong when it was written**. `bench_compose_frame_4k` has
existed since July, `#[ignore]`d, with a tracked baseline in
`bench/baselines.toml` under `[compositor_frame_4k]`. The survey behind the
claim checked the 108 recorded *runtime series* and stopped there; it did not
look in `bench/baselines.toml` and did not grep the source for `fn bench_`.

An afternoon was spent rebuilding it before anyone noticed, which is the cost
of an entry that states an absence confidently. The half that was genuinely
missing -- an assertion that the *partial* recomposite path is narrower than
the full one -- is now `gui/compositor/tests/damage_narrows_the_work.rs`, and
what else came out of the afternoon is written up under
`TD-C-A-4K-DESKTOP-FRAME-IS-OVER-THE-BUDGET`.

A `tests/` file rather than a criterion benchmark, for the reason the file
states: no gui crate has criterion, and a benchmark nobody runs measures
nothing. What it measured immediately is logged as
`TD-C-A-4K-DESKTOP-FRAME-MEASURES-SEVEN-TIMES-THE-BUDGET`.
**Update, 2026-09-12 (lane C).** Point 2 below -- "the runtime measurement
exists and is unchecked" -- was fixed on 2026-09-11 by giving
`the_demo_scene_still_composites` a real ceiling (`FRAME_CEILING_US =
50_000`) instead of an assertion that the clock runs. **Twenty-four hours
later it earned its keep.**

A workspace test failed with *a frame of this trivial scene took 67 074 us,
over the 50 000 us ceiling*. The first instinct was machine load -- the run was
a parallel workspace test on a busy desktop, and the recorded adjudication rule
says to suspect load. It was not load. Measured directly:

| | before | after the fix |
|---|---|---|
| `contrast_ratio` | 436 ns | 4 ns |
| `Palette::from_settings`, card theme | 87 395 ns | 3 102 ns |

The cause was the 4.5:1 text floor (§837) making palette resolution call
`contrast_ratio`, which was three `powf(2.4)` evaluations per colour -- and the
compositor calls `Palette::from_settings` **inside the render path**, per
blurred window, per frame. Fixed by tabling the sRGB transfer function: a
channel is a `u8`, so all 256 inputs are precomputed from the same formula, and
`guitk::theme::the_table_is_the_formula` asserts the table *is* the formula at
every one of them.

**What is worth generalising.** The bound was set with a comment explaining
where its number came from and a rule for adjudicating a failure. Both were
used within a day -- the number to see that 67 074 was 13x the median rather
than merely "slow", and the rule to decide whether to investigate. An
instrument with no threshold is a log line; a threshold whose value a later
reader cannot explain gets relaxed instead of investigated.

**Point 1 below is still open:** there is still no benchmark series for the
compositor, and lane A offered two shapes for one (a host-side criterion bench
in `gui/compositor/benches/`, or a ring-3 rung). This incident is an argument
for the host-side bench: the regression was in *host* code called from the
render path, and a criterion series would have shown it as a trend rather than
as a single ceiling breach that had to be diagnosed from scratch.

**A smaller thing this exposed, not yet fixed:** `Palette::from_settings` is
called per blurred window per frame rather than resolved once when the
appearance changes. At 273 ns it no longer matters for the budget, but
resolving a settings struct inside a render loop is the kind of thing that
stops being free the next time something is added to it.

---

### The original entry, for the record

**TD-C-THE-COMPOSITOR-FRAME-BUDGET-HAS-NO-INSTRUMENT** — as originally filed:

**Date:** 2026-09-11. **Lane:** C. Found by lane A while checking whether the
border conversion cost anything.

**In short:** `performance-targets.md` says a full desktop must be composited in
under 2 ms at 4K, or the machine misses a 144 Hz refresh. Nothing measures
whether it does. The compositor times its own frames and the only thing anyone
asserts about that number is that it is greater than zero.

**The two halves, because they are different problems:**

1. **No benchmark series.** The suite records 108 series — 35 of them `vfs`,
   then crypto, net, http, ipc, sched, page, heap and a handful of others — and
   **none** matching compositor, gui, draw, render, frame, blit, paint or
   surface. There is no `benches/` directory under `gui/` and no criterion
   dependency in any gui crate. Lane A checked this against the last
   Hyper-V/WHPX record rather than inferring it.
2. **The runtime measurement exists and is unchecked.** `gui/compositor/src/lib.rs`
   records `last_frame_time_us` every frame. `grep` finds exactly one assertion
   on it, at `lib.rs:9766`: `> 0`, with the message "the frame took no
   measurable time, so it did no work". That is a test that the clock runs, not
   that the budget is met. **This is the sharper half of the finding** — the
   instrument is already there and nobody reads it.

**What it means for the border conversion.** 436 draw sites moved from a fill to
a stroke through a different render path, and the tree would report the same
thing — silence — whether that cost nothing or ten times. Two things are now
pinned in `gui/appearance/src/surface.rs` so at least the question is answerable
without a benchmark:
`neither_theme_asks_the_compositor_for_more_work_than_the_other` (every
`Surface` kind emits the same command count in both themes, so the conversion
adds no commands) and
`an_outline_touches_far_fewer_pixels_than_the_fill_it_replaces` (a 100×40 box is
4,000 pixels filled and 276 outlined). Those bound the risk; they do not measure
the frame.

**Proper fix, and it is split across two lanes.** The harness is lane A's
(`bench/**` and the boot test) and they have offered to build that half. What a
meaningful compositor benchmark *composites* is a design question in this
subsystem and is mine: a plausible fixture is a full desktop at 4K — taskbar,
two overlapping windows with real content, a menu open — composed repeatedly,
reported as a series so `history.jsonl` can show drift. Neither half is useful
alone.

**Cheaper interim step, entirely in this lane:** raise that `> 0` assertion to a
real bound in a controlled case. It will not be 2 ms at 4K on a developer
machine under emulation — lane A measured run-to-run noise at a median 1.10x
with a p90 of 1.75x, so a tight bound would be flaky — but an order-of-magnitude
ceiling would catch a disaster, which is more than zero catches.

**If never fixed:** the one performance target this subsystem has, stated with a
number and a reason, remains unmeasured. Any change to the render path is
unfalsifiable, and the next one will not have someone asking the question.
