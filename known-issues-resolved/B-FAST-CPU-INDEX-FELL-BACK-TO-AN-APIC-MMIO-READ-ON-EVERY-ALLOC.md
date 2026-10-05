### B-FAST-CPU-INDEX-FELL-BACK-TO-AN-APIC-MMIO-READ-ON-EVERY-ALLOC. A self-inflicted allocator regression, and the benchmark that should have caught it never runs — 2026-08-14 — FIXED 2026-08-14

**Where:** `kernel/src/smp.rs` — `fast_cpu_index` / `current_cpu_index`.

**Self-reported.** Nothing failed; I found this by re-reading my own
`TD-FRAME-OWNER-1GIB` change against CLAUDE.md's performance-critical table.
It is logged rather than quietly patched precisely because it was invisible.

**What I did wrong.** `TD-FRAME-OWNER-1GIB` wired ownership tagging into the
allocator, so `alloc_frame` and `free_frame` each gained a call to
`frame_owner::current_owner()` → `smp::fast_cpu_index()`. On the boot-test CPU
model (`qemu64`) **neither RDPID nor rdtscp is advertised**, so every one of
those calls fell through to tier 3 — an uncached APIC MMIO read, hundreds of
cycles under emulation. `alloc_frame` is in the performance-critical table
(Linux buddy 100-500 ns, our target < 1 µs) and the recorded QEMU baseline for
`page_alloc_free` is 198 ns / 736 cycles, so this was a large relative cost on
a path CLAUDE.md explicitly says to benchmark after every change. I merged it
without benchmarking.

**The same fallback was also in ISR context.** `current_cpu_index` carried its
own hand-copied duplicate of the tier ladder, and the copy had drifted: it
never grew the RDPID tier, so on RDPID hardware it paid for a TSC read it threw
away, and on `qemu64` it took the APIC MMIO round-trip **on every timer tick**.
Its doc comment names the timer ISR as a hot path, which is what makes the
drift notable — the duplication defeated the optimisation exactly where the
comment claimed it mattered.

**Fix — tier 0, plus deleting the duplicate.** `fast_cpu_index` gained a
tier-0 fast path guarded by a new `MULTI_CPU_ACTIVE` flag: while no AP has ever
been released from the trampoline, exactly one CPU is executing kernel code, so
the answer is provably `BSP_CPU_INDEX` and no hardware read is needed at all.
`current_cpu_index` now delegates to `fast_cpu_index` after its
`SMP_INITIALIZED` gate, so there is one ladder instead of two and it cannot
drift again.

**The flag is deliberately not `NUM_CPUS_ONLINE > 1`,** which is the obvious
implementation and is unsound. An AP runs `gdt::init_for_ap`, `apic::init_ap`
and `spectre::init_ap` — all of which allocate — *before* it bumps that counter
(`smp.rs`, `ap_entry`). A counter-based test would therefore tell a live AP it
was the BSP and hand it the BSP's per-CPU allocator magazine: silent cross-CPU
corruption. Instead the **BSP** sets `MULTI_CPU_ACTIVE` before it sends the
first INIT-SIPI, so it is already true before an AP retires its first
instruction. It is monotonic and never cleared, because a stale `true` merely
costs the normal hardware read whereas a stale `false` is unsound.

**The real finding is why no one noticed.** The project has a benchmark suite,
`bench/baselines.toml` targets, and a pass/fail scorecard — and
`bench::run_all()` is spawned as a *deferred low-priority task* that prints
`BENCH_OK` only after `BOOT_OK`. The routine boot test waits for `BOOT_OK` and
kills QEMU immediately, so the benchmarks never finish. In the 26094-line KASAN
log, `[bench] === Kernel micro-benchmarks ===` is the **second-to-last line** —
the task started and was killed mid-suite. In the ordinary boot log the header
does not appear at all. So there was no gate to catch this, and there is none
for the next one either. `boot-test.sh --bench` (which waits for `BENCH_OK` and
surfaces `ABOVE TARGET` verdicts) exists but is not part of the routine gate.
Tracked separately as `TD-BENCHMARKS-ARE-NEVER-ACTUALLY-RUN-BY-THE-BOOT-GATE`.

**Regression guard added.** `bench::run_all` now measures `fast_cpu_index`
directly, with a `fast_cpu_index` entry in `bench/baselines.toml`. The target
(100 ns) is deliberately loose: it exists to detect "we fell back to the APIC
MMIO path", not to police single cycles. It is benchmarked not for its own sake
but because it is a *multiplier* — called twice per frame alloc/free and twice
per heap alloc/free — so a regression in it surfaces as a diffuse slowdown
across the whole allocator rather than as an obvious local fault, which is
exactly how this one hid.
