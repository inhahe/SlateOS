## TD-C-A-4K-DESKTOP-FRAME-IS-OVER-THE-BUDGET -- and most of this was already known

**STATUS 2026-09-13: the optimisation question is closed; what is left is the
GPU roadmap item.** The frame is still over the 2 ms target -- 6.88 ms at 4K
-- but the drawing code is at or below the memory-write floor and no
rearrangement of it can help. The measurements are at the end of this entry
under "already below the floor". The remaining work is
`roadmap.md`'s `[C]` "Wayland-inspired compositor: GPU acceleration, currently
a software rasterizer", which now carries the figure that motivates it.

This entry stays open as the record of *why* that is the remaining work, and
because the frame is genuinely over budget. It should not be picked up as an
optimisation task.

**Date:** 2026-09-13. **Lane:** C.

**Read this first.** This entry was written over an afternoon in which I
believed the compositor had no frame benchmark, because
`TD-C-THE-COMPOSITOR-FRAME-BUDGET-HAS-NO-INSTRUMENT` said so. It has one, and
has had since July:
`compositor::tests::bench_compose_frame_4k`, `#[ignore]`d, with a baseline in
`bench/baselines.toml` under `[compositor_frame_4k]` -- 2 ms target, **7.0 ms
measured**, and a recorded history of 48.6 -> 21.4 -> 15.8 -> 11.9 -> 10.6 ->
7.0 ms across five named optimisations. The earlier entry checked the 108
*runtime series* and concluded there was no benchmark; it never looked in
`bench/baselines.toml` or grepped the source. I trusted the entry instead of
the tree, which is the same mistake in a different costume.

**What that benchmark already says, that I spent the afternoon rediscovering:**

- the frame is over the 2 ms target and has been all along;
- the remaining gap is **memory-bandwidth bound on a full recomposite**;
- the per-pixel float-alpha cost was already hoisted out of the inner loops by
  a row-wise `fill_rect` rewrite -- which is why my clip-hoisting attempt
  (below) lost 30%: that family of optimisation is done;
- a timing benchmark must be `#[ignore]`d so it does not slow the correctness
  run -- stated in its doc comment, and re-learned here by breaking a
  neighbouring test;
- `bench_full_composite` exists precisely to bypass `should_compose`.

**What is genuinely new, and is what survives in the tree:**

1. `gui/compositor/tests/damage_narrows_the_work.rs` -- redrawing one window
   of eight costs about an eighth of a full recomposite. The absolute cost was
   measured; the *ratio* was not, and the ratio is the whole reason the
   partial path exists. It asserts a quotient, so it survives a shared
   machine, and it is cheap enough to run every time.
2. **A fixed trap.** `compose_frame` returns early on two paths and both left
   `last_frame_time_us` holding the previous frame's figure. A skipped frame
   therefore reported a stale number, and it did not look stale -- it looked
   like a beautifully repeatable measurement. Two separate probes read it as a
   result within the hour. Both paths set it to zero now, with a test.
3. **Shaping is not the cost.** Every `RenderCommand::Text` calls
   `font.shape()` in the draw loop and no shaping cache exists anywhere, which
   made it the obvious suspect for the ~60% of a frame that is text. Measured:
   755 ns for a 39-character string, so the scene's 240 text commands cost
   **181 us of a 13.8 ms frame -- 1.3%**. A shaping cache would buy nothing,
   and would have risked what the call site's own comment warns about: shaping
   happens there so the compositor lays text out exactly as the toolkit
   measured it, and a cache is a second layout path that can disagree.
4. **A negative result.** Hoisting the clip test out of the per-pixel glyph
   loop -- intersect the glyph rectangle with the clip once, iterate the
   survivors -- measured 27-34 ms against 19.5-26 ms as shipped, three runs
   each alternated on the same machine. Reverted. For a glyph nowhere near an
   edge, which is nearly all of them, the computed range is the range the old
   loop already walked.

**Where the time goes, measured.** Text is about 60% of a full recomposite
(20.3 ms with, 8.0 ms without, eight windows spread over 4K), and of that
~12.1 ms is putting ~9 400 glyph masks on the screen -- roughly 32 ns a
covered pixel.

**MEASURED 2026-09-13, and the 32 ns figure is not the blit.**
`bench_glyph_blit_phases` runs `draw_glyph` over a 12x16 mask with 128 covered
pixels, 20 000 glyphs scattered across the framebuffer, seven alternated rounds
with the minimum taken per phase:

| ns per covered pixel | 1080p | 4K |
|---|---|---|
| shipped (clipped blend) | 5.81 | 5.79 |
| the same with no clip | 4.61 | 4.63 |
| the same loop, direct store | 3.26 | 3.39 |
| **so: the per-pixel clip test** | 1.21 | 1.16 |
| **and the rest of `blend_pixel`** | 1.34 | 1.24 |

**Three things follow, and the first is the important one.**

1. **The blit costs about 5.8 ns a covered pixel, not 32.** Whatever the 12.1
   ms is, five sixths of it is not this loop.
2. **It does not move with the framebuffer size.** 4K and 1080p agree to
   within 2%, so the glyph loop is not the memory-bandwidth term -- an 8 MB
   buffer and a 33 MB one cost the same per pixel here.
3. **The whole per-pixel optimisation is worth at most 2.5 ns of 5.8**, which
   is 40% of the blit and about 7% of the figure it was meant to attack. The
   clip is tested twice per pixel (`draw_glyph` and again in
   `blend_pixel`'s `clip_allows`) and the index is looked up twice
   (`get` then `get_mut`); both are real and both are small.

**Where to look instead, from the same arithmetic.** 12.1 ms over ~9 400
glyphs is 1.29 us a glyph. This bench does 0.74 us a glyph while covering
*three times* as many pixels, and it starts from a mask that already exists.
So the missing time is **per glyph, not per pixel**.

**The first candidate has been measured and excluded.**
`osfont::system::tests::bench_warm_glyph_mask_lookup` times the cache hit that
hands the blit its mask -- 56 distinct glyphs, warmed first so it measures the
hit and not the rasteriser, seven rounds, minimum taken: **27.1 ns**. Over
9 400 glyphs that is **0.25 ms**, or 2% of the 12.1.

**So the budget now stands like this**, taking the entry's own figures (the
32 ns and the 12.1 ms imply ~378 000 covered pixels, ~40 a glyph):

| | measured | of 12.1 ms |
|---|---|---|
| the blit itself, 378 000 px at 5.8 ns | 2.2 ms | 18% |
| the warm mask lookup, 9 400 at 27.1 ns | 0.25 ms | 2% |
| **unaccounted** | **~9.6 ms** | **80%** |

**And the most likely explanation is the label, not the code.** 12.1 ms was
almost certainly obtained by differencing a frame with text against one
without, which includes *everything* text costs -- `font.shape` per command,
`FontCache::get` per command, `TextSpan::color_at` scanning the span list once
per glyph, the `RenderCommand::Text` iteration -- and not only "putting glyph
masks on the screen", which is what the entry calls it. Two measurements now
say the two things that phrase actually names come to a fifth of it.

**The split now exists and is measured directly, not by differencing.**
`RenderEngine` accumulates two nanosecond counters under `#[cfg(test)]` --
time in `font.shape` and time in `blit_run` -- and `bench_compose_frame_4k`
prints them per frame. At the *run* level, not the glyph level: two
`Instant::now()` calls cost tens of nanoseconds, which is the same order as
the glyph-cache hit they would be measuring one level down, and there are a
few hundred runs a frame against ten thousand glyphs.

**Its scene, three consecutive release runs, and they agree to 1%:**

```
compose_frame 4K (3840x2160, 16 windows): min=6.88ms
  phases: background_clear=1.33ms  window_render=5.51ms
  text:   shape=0.35ms             blit_run=0.29ms
```

**Read that carefully, because it is a different scene from the one above.**
The 20.3/8.0 ms figures were eight windows; this is the sixteen-window cascade
`bench_compose_frame_4k` has always used. The two are not comparable and this
does not show the earlier measurement was wrong. What it does show:

* **In this scene text is 0.63 ms of 6.88 ms -- about 9%.** Whatever is
  expensive here, it is not text.
* **Shaping is more than half of the text cost** (0.35 of 0.63), which is a
  different shape from "shaping is 1.3% of the frame" and worth keeping in
  view: the cheap half is the one with a cache.
* **The frame is 6.88 ms, and the recorded baseline agrees.** I wrote in the
  first version of this paragraph that `bench/baselines.toml` was "stale by a
  factor of two" and should be re-taken. **That was wrong, and I had not read
  the file when I wrote it.** `[compositor_frame_4k]` says
  `measured_ns = 7041000` -- 7.0 ms, dev host, release, 2026-08-16 -- which
  matches this measurement to 2%. The stale ~16 ms is in two *comments*: this
  bench's doc ("~15.8ms/frame release") and its catastrophe-guard note, both
  of which predate the 2026-08-16 improvement the baseline file recorded.
  Correcting a stale number by asserting a different file is stale, without
  opening it, is the same mistake this entry keeps documenting -- and it is
  left written down here rather than quietly fixed, because the pattern is
  the point.

**So the 2 ms target is missed by 3.4x, not by 8x**, and the thing to attack
in this scene is `window_render`'s 5.5 ms, of which text is one eighth.

### And that 5.5 ms is already below the floor, which settles the entry

`bench_fill_floor` fills exactly the bench's window area -- sixteen 1100x720
rectangles, 12.67 M pixels, 50.7 MB -- through the same `fill_rect` the
renderer uses. Three runs, minimum taken, agreeing to 1%:

| | time | written |
|---|---|---|
| opaque fill of that area | **5.92 ms** | 8.6 GB/s |
| the same area, alpha 0.5 (read-modify-write) | 25.8 ms | 2.0 GB/s |
| **`window_render`, same scene** | **5.51 ms** | -- |

**Window rendering is faster than a plain opaque fill of the same area.** It
has to be: the occlusion cull means it does not write all 12.67 M pixels. So
the drawing code is at or below the memory-write floor, and *no rearrangement
of it can help*. This entry has asserted "memory-bandwidth bound on a full
recomposite" since July on reasoning; it is measured now.

**Which makes the 2 ms target at 4K arithmetically unreachable on a CPU
compositor at this bandwidth**, and that is a statement about the target, not
about the code. One full-screen pass is 3840 x 2160 x 4 bytes = 33.2 MB; at
8.6 GB/s that is **3.9 ms to touch every pixel once**, before reading a single
source pixel. A 2 ms frame would need 16.5 GB/s of pure writes. The remaining
levers are therefore only two, and neither is in this code:

1. **Write fewer pixels.** Occlusion culling and damage tracking both already
   do this, and the damage path is why an idle desktop is cheap -- see
   `tests/damage_narrows_the_work.rs`. A *full* recomposite is the case where
   there is nothing left to cull.
2. **Do not write them with the CPU.** That is the GPU path, and it is a
   different subsystem rather than an optimisation of this one.

**The blended figure is the other actionable number here.** An alpha fill runs
at 2.0 GB/s against 8.6 -- four times slower, because every pixel is read as
well as written. Anything that makes a surface translucent (window opacity,
shadows, the blur behind a panel) pays that multiple over its area. That is a
design cost worth knowing before the next transparency feature, and it is
measured rather than assumed.

**A note on the estimator, because the first version of this bench got it
wrong.** Run once each in sequence, the three phases reported *"the clip test
is -0.98 ns"* -- a negative cost, which is a measurement saying the difference
between two of its numbers is smaller than its own noise. Alternating the
phases and taking each one's minimum over seven rounds is what made 1080p and
4K agree to 2%. A single ordered sample measures the order.

**What to do next.** Put a profiler on `draw_glyph`. Four hypotheses about
this frame have now been wrong -- that shaping was the cost, that the
compositor was 7x over budget (it was my scene, with every window stacked at
the origin), that hoisting the clip would help, and that no benchmark existed.
Every one was cheap to disprove by measuring and expensive to act on. Reading
the inner loop has been wrong every time.
