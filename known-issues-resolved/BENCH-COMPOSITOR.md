### BENCH-COMPOSITOR. Compositor frame benchmark — RESOLVED 2026-07-01 (benchmark added; revealed BENCH-COMPOSITOR-SLOW)

**Resolution:** added `bench_compose_frame_4k` (an `#[ignore]`d measurement test
in `gui/compositor/src/main.rs`) plus the `Compositor::bench_full_composite`
hook (which shares `full_recomposite_into_back` with the real `compose_frame`
so they can't drift) and the `[compositor_frame_4k]` baseline in
`bench/baselines.toml`. The compositor is host-runnable (`cargo test -p
compositor --target x86_64-pc-windows-gnu --release -- --ignored --nocapture
bench_compose_frame_4k`), so a real number is measurable. Running it immediately
surfaced the ~25x-over-target result now tracked as BENCH-COMPOSITOR-SLOW above.
Original gap description retained below for context.

**Where (original gap):** `gui/compositor/src/main.rs` — the composite path is
`Compositor::compose_frame` (line ~2746) → `render_all_windows` (~2807) →
`blit_buffer` (~2949). There is frame-budget *tracking* at runtime
(`end_frame`, line ~849, returns whether the frame was within budget) but no
benchmark that measured the actual composite cost against a target.

**What:** CLAUDE.md's performance-critical-subsystems table lists "Compositor
frame — Must composite a full desktop in < 2ms at 4K to not miss 144Hz vsync"
as a hard benchmark target, and mandates "benchmark everything critical." Every
other critical subsystem (syscall dispatch, IPC, context switch, page fault,
page/heap alloc, scheduler pick_next, futex, io_uring, IOCP, ISR latency, VFS,
FS r/w) has a benchmark in `kernel/src/bench.rs` scored against a
`bench/baselines.toml` target. The compositor has none. `bench/` currently
contains only `baselines.toml` (no per-subsystem benchmark crates yet), and
`grep` finds no `criterion`/`#[bench]`/`fn bench` anywhere under `gui/`.

**Why not done in the discovering session:** identified during a benchmark-gap
audit at the tail of a long, context-heavy autonomous session. Doing it right
(build a host- or target-runnable harness that constructs a 4K in-memory
framebuffer + a representative multi-window damaged scene, drives
`compose_frame`/`render_all_windows`, and records ms/frame against the 2ms
target) is real work that deserves a fresh context rather than a rushed pass.

**Proper fix:** add a compositor composite-frame benchmark. Options:
(a) a `criterion` bench under `gui/compositor/benches/` if the compositor crate
(deps: `guitk`, `guiremote`) builds and composites on the host with an
in-memory framebuffer (verify `compose_frame`/`render_all_windows`/`blit_buffer`
don't require real DRM/KMS hardware handles — construct the `Compositor` with a
plain `Vec`-backed 3840×2160 framebuffer); or (b) if the composite path is too
coupled to the target, add an in-kernel/target self-test bench analogous to
`bench_pick_next_scaling`, driving a synthetic scene and using `rdtsc`. Scene
should scale window count / damage area to expose O(n)-in-pixels or
O(n)-in-windows behaviour. Record a `[qemu.compositor_frame_4k]` (and/or a
host baseline) in `bench/baselines.toml` with `target_ns = 2_000_000` (2 ms).
Note the compositor is userspace, so the on-hardware number (not the TCG figure)
is the meaningful one; document the measurement environment.

**Trigger:** next time the compositor's render path is touched, or as the next
benchmark-infrastructure task — it is unblocked (does not need Linux binaries or
operator input), just deferred for context reasons.
