### BENCH-COMPOSITOR-SLOW. Compositor over its 4K frame budget (~7.0ms/frame vs 2ms) — PERF BUG 2026-07-01, IMPROVED 6.9x 2026-08-16

**UPDATE 2026-08-16 (6) — inter-window occlusion cull landed, 12.0ms → 7.0ms/frame
min (cumulative 48.6ms → 7.0ms = 6.9x). It also corrects a wrong assumption
recorded in UPDATE (5) below.**

*First, the measurement that redirected the work.* UPDATE (5) closed with
"the 4K benchmark's dominant cost is the background clear + per-window
RenderEngine draws", and the deferred next step recorded there (and in
`requests/a-c-bench-compositor-entries-are-yours.md`) was a persistent tile
thread-pool to amortize the per-frame `thread::scope` spawn. That was aimed at
the wrong half. A new `Compositor::bench_full_composite_phases()` returning
`(background_clear_ns, window_render_ns)` measured the split directly:

| phase | before | after |
|---|---|---|
| background clear | 1.531 ms | 1.392 ms |
| window render | 11.128 ms | 5.313 ms |
| whole frame (min / mean) | 12.046 / 15.231 ms | 7.041 / 8.955 ms |

The clear was **already fine** — 13% of the frame, and the three rounds of
parallel-fill work above had done their job. Window rendering was 88%. So the
bottleneck was not a parallelism problem at all; it was **overdraw**: windows
were painted strictly back-to-front, so every pixel of every window was drawn
even where a higher window overwrote it. Parallelizing that would have divided
the wasted work across cores instead of not doing it. **Lesson worth keeping:
the "remaining work" note at the end of a perf entry is a hypothesis, not a
finding — measure the split before acting on it.**

*The fix.* Each window's conservative drawn extent (client rect + `BORDER_WIDTH`
+ `SHADOW_SIZE` + 3 slack, plus `TITLE_BAR_HEIGHT` on top) now has the
provably-opaque covers of all windows **above** it subtracted from it, and the
window is redrawn once per surviving fragment under a framebuffer-level clip.
Pieces:

- `Rect::subtract` — exact rectangle difference. It cuts the top and bottom
  bands **full-width first** and only then splits the middle row into left/right.
  That ordering is load-bearing: the fragments must be **disjoint**, because a
  window is redrawn once per fragment, so a pixel appearing in two fragments is
  painted twice — invisible for an opaque fill, but *wrong* for a translucent
  one (the shadow would darken along the seam).
- `subtract_region(base, occluders, max_parts) -> Option<Vec<Rect>>` — declines
  (returns `None`, caller falls back to one unclipped draw) rather than
  fragmenting without bound. `MAX_FRAGMENTS = 4`: past that, the per-fragment
  redraw setup costs more than the overdraw saved.
- `window_opaque_cover` / `window_drawn_extent` — the per-window predicates,
  factored out and now **shared with the background cull** (`opaque_cover_rects`
  was refactored onto them), so the two culls cannot disagree about what counts
  as opaque.
- `Framebuffer::frame_clip` + `set_frame_clip` — the enforcement point. It lives
  on the framebuffer rather than in `RenderEngine`'s clip stack because **three**
  routes paint a window — the render engine's commands, the decoration helpers
  (`render_shadow`/`render_border`/`render_title_bar`, which run *outside*
  `execute` where the clip stack is cleared), and the shared-buffer blit — and
  only the framebuffer is common to all three. A cull honoured by just one route
  would silently let a hidden window's decorations through. Every primitive that
  writes a pixel now goes through `clip_allows` (per-pixel) or `clip_span`
  (per-row); `copy_row` and `blit_opaque_band` shift the *source* offset in step
  with the narrowed destination. The clear/`clear_except` path is deliberately
  **not** clipped — it runs once per frame before any window and has its own cull.

*Verification, and why it is not vacuous.* `Compositor::occlusion_cull: bool`
(default `true`) exists purely so a test can composite the same scene both ways:
`occlusion_cull_composites_the_same_pixels_as_drawing_every_window` builds a
900×700 six-window cascade — **window 3 deliberately translucent at 0.5**, which
is the case a non-disjoint subtraction would corrupt — twice, and compares front
buffers pixel for pixel. Plus
`subtract_yields_disjoint_parts_that_miss_exactly_the_occluder` (exhaustive
per-pixel coverage-count oracle over 6 occluder shapes: interior, disjoint,
covering, edge bands; asserts every pixel is covered exactly 0 or 1 times and
the areas sum) and
`subtract_region_declines_rather_than_fragmenting_without_bound`. To prove these
actually bite, a deliberate bug was injected (`if false &&` on the left-fragment
push in `Rect::subtract`): **both** new tests failed, the equivalence test
reporting "occlusion cull changed 85246 pixel(s)". 82 compositor tests, clippy
clean.

**Still open, and what to try next.** 7.0 ms against a 2.0 ms target — 3.5x to
go, so this entry stays. The honest reading of the new phase split is that
window render is *still* the larger half (5.3 of 7.0 ms) even with the overdraw
gone, so the next attempt should again start by measuring rather than assuming.
Candidates, in the order I would try them: (a) SIMD/streaming stores for the
remaining large opaque fills — the frame is now much closer to a pure
memory-bandwidth problem than it was, which is exactly when non-temporal stores
pay; (b) damage tracking, i.e. recompositing only the changed region across
frames rather than a full desktop every time — a much bigger structural win than
anything left inside a single frame, and the natural next design step; (c) the
persistent tile thread-pool from UPDATE (5), which is now worth *less* than it
looked (the work it would parallelize has shrunk by half) but is still real.

**`bench/baselines.toml` needs `measured_ns` 10572000 → 7041000** — that file is
`bench/**`, i.e. lane A's zone, so it is filed as `requests/c-a-bench-baseline-compositor-frame-4k.md`
rather than edited here.

**UPDATE 2026-07-14 (5) — parallel opaque window blit landed (first increment of
the deferred window-render parallelization).** The opaque fast path of
`Compositor::blit_buffer` (full-opacity window carrying an `is_opaque()` Xrgb
shared buffer — the common game/video/maximized-window case, which ran every
frame as O(rows) serial `copy_row` memcpys) is now parallelized across
destination row-bands, mirroring the existing `clear_except` band split. New
`Framebuffer::blit_opaque(buf, win_x, win_y, cols, rows)` partitions the back
buffer into disjoint `chunks_mut` row-bands filled on `std::thread::scope`
workers (no `unsafe`, no aliasing; `&SharedBuffer` is `Sync` so workers share it
read-only via `buf.row(r)`), gated by the same `fill_worker_count` heuristic (1M
px threshold, cap 8, single-thread fallback). The static
`blit_opaque_band(band, by0, band_rows, fb_width, buf, win_x, win_y, cols, rows)`
helper replicates `copy_row`'s clipping (left `src_off` when `win_x<0`,
right-edge `min`, vertical band ownership) byte-for-byte, so the result is
bit-identical to the old serial path. Two new unit tests:
`test_blit_opaque_matches_serial_reference_large` (2048×1024 fb > threshold,
1200×900 buffer at 5 offsets incl. negative and offscreen; asserts parallel ==
serial reference) and `test_blit_opaque_clips_edges_small` (single-thread path,
all clip corners). 66 compositor tests total, clippy clean. NOTE: only the
*opaque* blit is parallelized; the per-pixel alpha-blend path stays
single-threaded, and this still spawns a fresh `thread::scope` per blit — the
remaining gap needs a persistent thread pool (to amortize spawn cost) and to
parallelize the `RenderEngine` per-window content draws (not just the final
buffer blit). baselines.toml unchanged (the 4K benchmark's dominant cost is the
background clear + per-window RenderEngine draws, not buffer blits; this helps
buffer-backed-window workloads specifically).

**UPDATE 2026-07-02 (4) — parallel background clear landed, 11.9ms → 10.6ms/frame
min (cumulative 48.6ms → 10.6ms = 4.6x).** Both `Framebuffer::clear` and
`Framebuffer::clear_except` now split the framebuffer into horizontal row-bands
and fill them concurrently via `std::thread::scope` over disjoint
`chunks_mut` slices — no `unsafe`, no shared mutable aliasing (each worker owns a
distinct slice). Worker count comes from the new `fill_worker_count`, which caps
at 8 and gracefully falls back to a single thread when the buffer is below 1M
pixels or when `std::thread::available_parallelism()` can't be reported, so it
never pessimizes small buffers or single-core targets. The per-scanline
span-merging logic (formerly inline in `clear_except`) was extracted into the
static `fill_uncovered_band(buf, y0, band_rows, width, color, covered, fb_height)`
helper, shared by the single-threaded and parallel paths, using absolute-y
overlap tests against the covered rects and band-local writes. New unit test
`test_clear_except_parallel_band_boundaries` (2048×1024 = 2M px, covered rects
straddling band boundaries; asserts the parallel result is byte-identical to the
single-threaded reference, plus covered-kept / uncovered-cleared spot checks). 64
compositor tests total, clippy clean. baselines.toml `measured_ns` updated to
10572000. NOTE: this only parallelizes the *background clear*; the per-window
opaque content draws are still single-threaded, so the remaining gap needs a
persistent thread-pool (to amortize the per-frame `thread::scope` spawn cost) + a
RenderEngine band-view refactor to parallelize the window-render tiles too.

**UPDATE 2026-07-02 (3) — desktop-clear occlusion cull landed, 15.8ms → 11.9ms/frame
min (cumulative 48.6ms → 11.9ms = 4.1x).** The full-desktop background clear no
longer memsets the pixels hidden behind opaque windows. `full_recomposite_into_back`
now computes `Compositor::opaque_cover_rects()` — the screen-space rectangles
provably overwritten with opaque content this frame (buffer-less windows whose
first command opaquely covers the client area at full opacity, and windows
carrying an opaque `is_opaque()` shared buffer at full opacity, over the covered
sub-rect) — and calls the new `Framebuffer::clear_except(color, &covered)`, which
fills only the complementary (uncovered) spans per scanline. Decorations
(title bar, border, translucent shadow) are deliberately excluded from the cover
rects since they lie outside the client rect, so the background under them is
still cleared (conservative → only ever costs a little correct overdraw, never
correctness). New unit tests: `test_clear_except_*` (4: empty/single/overlapping-
merge/offscreen-clip), `test_opaque_cover_rects_*` (3: opaque-command window
reported, translucent/minimized/rounded excluded, buffer sub-rect + Argb
excluded), and `test_full_recomposite_cull_matches_uncovered_background` (visual
equivalence). 63 compositor tests total, clippy clean. baselines.toml
`measured_ns` updated to 11929000.

**UPDATE 2026-07-02 (2) — occlusion cull landed, 21.4ms → 15.8ms/frame min
(cumulative 48.6ms → 15.8ms = 3.1x).** `render_window` now skips the
compositor's default white client-background fill when the client's first render
command is an opaque, square-cornered `FillRect` that fully covers the client
area on a fully-opaque window (`Compositor::first_command_covers_client`). That
first fill was 100% overdraw in the common "client paints its own background"
case (~29% of the 4K benchmark's opaque stores). Guarded to be correct: rejects
translucent windows (opacity < 1.0), non-opaque colors (alpha < 255), rounded
corners (corner pixels would show the bg), and partial-cover rects. New unit
test `test_first_command_covers_client` (55 tests total). baselines.toml
`measured_ns` updated to 15831000.

**UPDATE 2026-07-02 (1) — fill_rect row-wise rewrite landed, 48.6ms → 21.4ms/frame
min (2.3x).** `RenderEngine::fill_rect` no longer calls `blend_pixel`
per pixel. Two new `Framebuffer` fast paths were added next to `copy_row`:
`fill_row_solid` (opaque color → single `[u32]::fill`/memset per row, skips the
per-pixel float-alpha math and bounds check) and `blend_row` (translucent color
→ hoists the alpha computation and branch out of the inner loop, integer blend
only). `fill_rect` resolves the effective alpha once (color-alpha × opacity) and
dispatches to the solid, blend, or skip (alpha 0) path per row.

**Why it's still over 2ms (and why the remaining gap is *not* another naive-code
bug):** after culling the wasted white bg fill, the benchmark still issues ~31M
opaque u32 stores/frame — an 8.3M-pixel clear plus 16 windows painting opaque
client content — i.e. ~124 MB written per frame. At ~16ms that's ~8 GB/s
effective, near the ceiling for scalar cache-polluting stores on this host. The
per-pixel-work bug is fixed; what's left is memory bandwidth on a *full*
recomposite. Getting a full 16-window 4K
recomposite under 2ms would need SIMD non-temporal (streaming) stores +
multithreaded tiles, and/or occlusion culling to skip the fully-covered white
client-bg fill (that first fill is 100% overdraw when the client paints an opaque
full-window rect). **Crucially, steady-state rendering does NOT full-recomposite
every frame** — the compositor uses damage-rect partial updates (only changed
regions repaint), which is the actual 144Hz-vsync mechanism; this benchmark
deliberately stresses the worst-case full-recomposite path (wallpaper change,
resize, many simultaneously-moving windows). Remaining optimization directions
below are now *lower priority* — the dominant per-pixel bug is resolved.

**Where:** `gui/compositor/src/main.rs` — the software composite path
`Compositor::full_recomposite_into_back` → `render_all_windows` (~2807) →
`render_window` (~2832, shadows + decorations + per-command draw) over the
`Framebuffer` per-pixel ops (`clear`/`clear_rect`/`set_pixel`/`blend_pixel`,
~503-600).

**Measured (2026-07-01, via the new `bench_compose_frame_4k`):** a 4K
(3840×2160) full recomposite with 16 decorated windows carrying toolkit client
content takes **~48.6ms/frame (min), ~50ms mean, RELEASE build on the dev
host** — roughly **25x the 2ms target** in CLAUDE.md's perf-critical table,
i.e. ~20fps, missing even a 60Hz (16.7ms) vsync budget, nowhere near 144Hz
(6.9ms). This is the classic "correct-but-naive" hot-path code the
benchmark-everything mandate exists to catch. Recorded in
`bench/baselines.toml` `[compositor_frame_4k]` (`measured_ns = 48570000`).

**Likely culprits (profile before optimizing):** (1) ~~per-pixel scalar fills in
`fill_rect` with bounds-checks per pixel~~ — **FIXED 2026-07-02** (row-wise
`fill_row_solid`/`blend_row`); (2) ~~per-pixel float alpha in `blend_pixel` for
solid fills~~ — **FIXED for fills** (alpha resolved once per fill; `blend_pixel`
still used by the per-pixel `blit_buffer` slow path and font glyphs); (3)
full-screen clear + full redraw of every window every frame even when
`bench_full_composite` forces it — the real `compose_frame` has a partial-damage
path, but the fully-damaged case (wallpaper change, resize, many moving windows)
hits this — STILL the structural cost (bandwidth-bound overdraw); (4)
`render_window` clones `render_tree.commands` and the z-stack each frame
(`render_all_windows`/`render_window`, allocations on the hot path) — small for
the benchmark's 4-command windows, but worth eliminating for large trees.

**Remaining optimization directions (lower priority — per-pixel bug resolved):**
SIMD non-temporal/streaming stores for solid rects (avoid cache pollution on
huge fills) + multithreaded tile compositing to break the single-core bandwidth
ceiling; ~~occlusion culling so a window's default opaque client-bg fill is
skipped when the first command fully covers it~~ — **DONE 2026-07-02** (first-command
cull, plus desktop-clear cull under fully-opaque covering windows and opaque
shared buffers — DONE 2026-07-02 (2) & (3)); precompute/caches for window
decorations and shadows (they rarely change frame-to-frame); avoid per-frame
`Vec` clones in `render_window` (borrow or reuse scratch buffers); ensure the
damage-tracking fast path is actually taken for the common "one window changed"
case. Target: < 2ms/4K (for a full recomposite; likely needs SIMD+threads). NB:
this is the CPU-software fallback; the eventual GPU/DRM-KMS accelerated path is
separate.

**Status:** per-pixel-cost bug FIXED + redundant-bg-fill occlusion cull DONE +
desktop-clear occlusion cull DONE + parallel background clear DONE (cumulative
4.6x, 48.6ms → 10.6ms, 2026-07-02); the remaining gap to 2ms on a *full*
recomposite is memory-bandwidth-bound (~124 MB/frame worst case at ~12 GB/s
scalar stores) and needs a SIMD-streaming-store + multithreaded-window-tile
initiative (its own focused session: persistent thread-pool to avoid per-frame
`thread::scope` spawn cost + a RenderEngine band-view refactor). All the cheap
algorithmic overdraw wins have now been taken; the remaining work is a
bandwidth/parallelism problem, not a naive-code problem. Unblocked (no Linux
binaries / operator input needed).
