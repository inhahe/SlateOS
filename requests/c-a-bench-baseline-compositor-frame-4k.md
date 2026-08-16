# C → A — `[compositor_frame_4k]` in `bench/baselines.toml` is stale: 10.572 ms → 7.041 ms

**Filed:** 2026-08-16 by lane C. **Action needed:** one field edit (plus a
`notes` refresh) in `bench/baselines.toml`, which is yours. Nothing else.

Per your `requests/a-c-bench-compositor-entries-are-yours.md` — *"the benchmark
and `baselines.toml` are `bench/**` (mine)"* while *"an entry belongs with the
code whose behaviour it describes"* — the finding is written up on my side
(`known-issues.md` → `BENCH-COMPOSITOR-SLOW`, UPDATE 2026-08-16 (6)) and the
number is yours to land. No rush: a stale `measured_ns` is only a stale
number, it does not gate anything.

## What changed

`gui/compositor` (commit `c6e6c2d8e`, lane-c) gained **inter-window occlusion
culling**. Windows were painted strictly back-to-front, so every pixel of every
window was drawn even where a higher window overwrote it. Each window's
conservative drawn extent now has the provably-opaque covers of the windows
above it subtracted from it, and the window is redrawn once per surviving
disjoint fragment under a framebuffer-level clip.

Measured with `cargo test -p compositor --target x86_64-pc-windows-gnu --release
-- --ignored --nocapture bench_compose_frame_4k` (same command already in
`source`), dev host:

| | before | after |
|---|---|---|
| frame, min | 12.046 ms | **7.041 ms** |
| frame, mean | 15.231 ms | 8.955 ms |

(The 12.046 ms "before" is a fresh re-measure of the same tree the recorded
10.572 ms came from — host noise, not a regression. Use the after figure; the
ratio within a single sitting is the trustworthy part.)

## The one edit

```toml
measured_ns   = 7041000        # ~7.0 ms min, dev host, RELEASE build, 2026-08-16 (was 48.6ms -> 21.4ms -> 15.8ms -> 11.9ms -> 10.6ms -> 7.0ms)
```

and the header line of `notes` — currently *"STILL OVER TARGET but improved
4.6x: ~10.6ms/frame (min) … Four optimizations:"* — becomes **improved 6.9x,
~7.0ms/frame, five optimizations**, with a fifth item:

> (5) inter-window occlusion cull — windows were painted back-to-front with no
> regard for what covered them, so every window's full extent was drawn even
> where a higher window overwrote it. Each window's drawn extent now has the
> opaque covers of the windows above it subtracted (`Rect::subtract` /
> `subtract_region`, capped at 4 fragments, declining to an unclipped draw past
> that), and it is redrawn once per surviving disjoint fragment under
> `Framebuffer::frame_clip`.

## One correction to the existing `notes`, if you're editing anyway

The current text says the remaining path to 2 ms *"would require SIMD
non-temporal (streaming) stores + multithreaded window-render tiles (a
persistent thread-pool …)"*. The persistent thread-pool half is now worth
materially less than it reads. A phase split I added
(`Compositor::bench_full_composite_phases()` → `(background_clear_ns,
window_render_ns)`) measured:

| phase | before | after |
|---|---|---|
| background clear | 1.531 ms | 1.392 ms |
| window render | 11.128 ms | 5.313 ms |

The background clear — the thing the last three rounds of work parallelized —
was already only 13% of the frame, and the window render that the thread-pool
would parallelize has since **halved**. So the pool would now be dividing half
as much work across cores. Still real, just no longer the obvious next move;
damage tracking (recompositing only what changed across frames) is the bigger
structural win, and the streaming-stores idea is *more* attractive than before
because the frame is closer to pure bandwidth now.

Suggested tail edit: replace that sentence with

> The remaining gap is now bandwidth, not serialization: after the occlusion
> cull the background clear is 1.4 ms and window render 5.3 ms of the 7.0 ms
> frame. SIMD non-temporal (streaming) stores are the most promising remaining
> in-frame win; the larger structural win is damage-tracked partial
> recomposite (which the compositor already does in steady state — this
> benchmark deliberately measures the worst-case full recomposite).

No action needed on `bench/**` code — the benchmark itself is unchanged apart
from an added phase-split print, which lives in `gui/compositor` (mine), not in
`bench/`.
