### B-DF1. Kernel-stack overflow → double fault when an IRQ frame pushes onto a near-full kernel task stack (deferred benchmark suite) — FIXED 2026-06-15 (Q7 option A)

**RESOLVED 2026-06-15.** Fixed via `open-questions.md` Q7 → **option A**
(operator-chosen): a dedicated per-CPU guard-page IRQ stack with a manual
nesting-aware switch in `idt::irq_common_dispatch` (so hardware IRQ frames/
handlers never consume the interrupted task's stack), plus **deferred
preemption** (timer ISR sets `NEED_RESCHED`; the outermost IRQ frame runs the
context switch on the task stack via `sched::do_deferred_preempt`). The
restructuring also exposed an **unbounded re-entrant preemption recursion**
(nested timer tick during `schedule_inner`, with interrupts enabled on the task
stack, misclassified as a fresh outermost IRQ → recursion until guard-page
overflow); fixed by disabling interrupts across the involuntary switch in
`do_deferred_preempt`. See `design-decisions.md` §26. **Validated:**
`http_gzip_8KiB` — which previously double-faulted entering the dashboard benches
on a near-full task stack — now runs to completion.

**Follow-up 2026-06-15 — `BENCH_OK` now reached end-to-end.** After the Q7
landing, two further blockers were chased to ground:

1. **The previously-documented `bench_isr_latency` null-pointer crash no longer
   reproduces.** It was an artifact of the *old* timer-ISR path that called
   `preempt()` inline during the hard-IRQ handler; the Q7 deferred-preempt
   restructuring (timer ISR only sets `NEED_RESCHED`; the switch runs later on
   the task stack) removed it. Verified by running `bench_isr_latency()` both
   early and in its normal end-of-suite slot — it completes cleanly (≈54 µs
   hard-IRQ phase under TCG, above the 10 µs target but that is emulation
   noise, not a fault). The stale `todo.txt` "Cross-Zone Bug Reports" entry is
   superseded.

2. **The actual last `BENCH_OK` blocker was a scheduler self-deadlock, now
   fixed.** `bench_dashboard_api_status` calls `dashboard::api_status()` →
   `sched::task_list()`, which holds `SCHED` (a plain `spin::Mutex`) across a
   heap `Vec` collect over *all* tasks. Run 1000× in a tight loop, a timer tick
   reliably lands while the task holds `SCHED`; the Q7 deferred-preempt then ran
   `preempt() → schedule_inner() → SCHED.lock()` on the *same* CPU and spun
   forever (the `cli` in `do_deferred_preempt` made the hang unrecoverable). The
   fix: `do_deferred_preempt` now checks `SCHED.is_locked()` and, if held,
   re-arms `NEED_RESCHED` and defers to the next tick instead of blocking — the
   same try/skip discipline `unthrottle_expired()` already uses from ISR
   context. This closes the *entire* "involuntary preempt while the interrupted
   context holds SCHED" deadlock class (including the tiny analogous window
   during voluntary `yield_now`/`block`), at the single involuntary-preempt
   site. **Validated: the full `--bench` suite now reaches `BENCH_OK` ("Boot
   test PASSED").** See `design-decisions.md` §27.

The original analysis is retained below for history.

**Root cause (CONFIRMED): kernel task stack overflow into the guard page.**
The deferred benchmark suite runs heavy, *debug-built* code paths in kernel
context (gzip/deflate, `format!`-heavy JSON, crypto) on a kernel task with a
fixed **64 KiB** stack (`TASK_STACK_SIZE = 4 * 16 KiB`). The kstack allocator
(`kernel/src/mm/kstack.rs`) lays out each task stack as `[guard 16 KiB][stack
64 KiB]`, slot stride `SLOT_SIZE = 0x14000`, region base `0xFFFF_C100_0000_0000`.
The reported fault `RSP = 0xffffc1000003ffb8` decodes to slot 3, within-slot
offset `0x3FB8`, which is **< GUARD_SIZE (0x4000)** — i.e. RSP is **inside the
guard page**, ~72 bytes below `stack_bottom`. So the stack overflowed; the
faulting `atomic_load` (and the IRQ frame that the CPU was pushing) landed on
the unmapped guard page → the fault could not be delivered → #DF.
(Correction to an earlier note: RSP is **not** "near the top of the stack" — I
had mis-decoded the slot stride. It is firmly in the guard page. The two
backtrace frames are the #DF handler's own IST stack — `handle_double_fault` /
`isr_double_fault` — and are uninformative.)

**Why an IRQ tips it over.** Hardware IRQs (timer vector 32; device IRQs 33–56,
incl. mouse IRQ12) are installed in the IDT with **IST index 0** (see
`idt.rs::init`, `IdtEntry::new(..., 0, 0)`) — they run on the *current* kernel
task stack, not a dedicated stack. When a benchmark has driven the task stack
near `stack_bottom`, the CPU pushing the interrupt frame (and the handler's own
frames) crosses into the guard page → #DF. Only the double fault itself uses an
IST (IST1). This makes *any* near-full kernel stack a double-fault risk on the
next interrupt — a real, production-relevant bug for any in-kernel code that
uses a lot of stack, not merely a benchmark artifact.

**FIXED part — the 16 KiB gzip hash table (`kernel/src/fs/compress.rs`).**
`lz77_tokenize()` allocated `let mut head = [0u32; HASH_SIZE]` with
`HASH_SIZE = 4096` = **16 KiB on the stack** (a quarter of the whole 64 KiB
stack), while its sibling `prev` was already heap-allocated. Moved `head` to a
`Vec` (heap) and changed `insert_hash`/`find_best_match` to take `&[u32]`/`&mut
[u32]` slices (call sites unchanged — `&mut Vec<u32>` coerces). Verified: with
this fix the `http_gzip_1KiB` and `http_gzip_8KiB` benchmarks now **complete**
(8192B → 4507B), where before they double-faulted. This was the dominant
single stack frame and removing it is correct regardless (gzip should never use
16 KiB of stack).

**OPEN part — RESOLVED 2026-06-15 by the Q7 option-A per-CPU IRQ stack;
empirically confirmed 2026-06-20.** The systemic interrupt-on-near-full-stack
overflow was fixed by moving interrupt handling off the interrupted task's stack
onto a dedicated per-CPU guard-page IRQ stack (`idt.rs::init_irq_stack` /
`run_on_irq_stack` / `IRQ_STACK_TOP`/`IRQ_STACK_BOTTOM`, with nesting-aware
manual RSP switch + `sched::do_deferred_preempt` after RSP is back on the task
stack — see open-questions.md Q7 / design-decisions.md §26). Once IRQ frames no
longer land on a near-full task stack, the 64 KiB task stack is sufficient for
the debug-built `core::fmt`-heavy dashboard path. **Validated 2026-06-20:**
`scripts/boot-test.sh --bench` runs the *entire* deferred suite to completion —
`dashboard_api_status`/`_health`/`_metrics`, `isr_latency`, the 62-entry
scorecard, and a clean `BENCH_OK` — with no double fault (serial-test.txt lines
9843–9913). The stale "still double-faults entering dashboard_api_status"
description below is retained for history only and no longer reproduces.

_Historical (pre-fix) description:_ After the
gzip fix the suite advances one stage further and double-faults again at the
**identical** guard-page `RSP=0xffffc1000003ffb8`, now in `Task 114` during
`bench_dashboard_api_status` (`crate::net::dashboard::bench_api_status`). The
dashboard path has no single large array — it is `format!`-heavy, and debug
builds give `core::fmt` very deep, un-inlined, stack-hungry call chains. So this
is the *general* problem: 64 KiB is marginal for debug-built in-kernel heavy
code + an IRQ frame on top. Fixing it benchmark-by-benchmark is whack-a-mole.

**Proper fix is an architectural decision — see `open-questions.md`.** The
textbook fix is a dedicated per-CPU IRQ stack (x86 IST), like Linux's IRQ
stacks, so interrupt handlers never consume the interrupted task's stack.
**Complication:** the timer handler deliberately re-enables interrupts
(`apic.rs:1162`, `sti` after EOI, for preemption), so IRQs *can* nest — a naive
single shared IRQ IST would be clobbered by a nested IRQ resetting RSP to the
IST top. A correct IRQ-stack implementation must therefore support nesting (or
the hard-IRQ phase must not re-enable IF). This is a careful change to the
hottest, most safety-critical path; alternatives (bump kernel-task stack size;
keep heavy code out of the kernel; release-build) each have tradeoffs. Deferred
to the operator as an open question rather than changing the IRQ path
autonomously.

**Reproduce:** `bash scripts/boot-test.sh --bench --timeout=600`; the suite now
runs through `compress`, `context_switch`, `pick_next`, `ipc`, `vfs`, all
`http_*` incl. both `http_gzip_*`, then #DFs entering `dashboard_api_status`.

**Large-stack-array audit (2026-06-14).** I scanned the kernel for fixed-size
stack arrays ≥ 8 KiB that could contribute to the same overflow class. Findings:
`bench.rs::bench_vfs_throughput_16k` held a `[u8; 16384]` (16 KiB) in the bench
task — moved to a heap `Vec` (committed). Remaining latent (lower-risk, not the
immediate trigger, left as tech-debt): `audio_notify.rs::self_test` `[u8; 8192]`
(boot self-test path), `syscall/linux.rs` ~line 53451 `drain [u8; 8192]`, plus
several `[u8; 4096]` buffers in `rng`/`smp`/`virtio/sound`/`linux.rs` self-tests.
Note these arrays are **not** the immediate dashboard double fault: the
`dashboard_api_status` overflow has **no** large array — it is pure debug-built
`core::fmt` call-chain depth — so reducing stack arrays will not by itself make
`BENCH_OK` appear; only the Q7 IRQ-stack / stack-size decision will.

**Impact (historical):** Before the Q7 IRQ-stack fix, `BENCH_OK` and the last
benchmarks (dashboard API, ISR latency, scorecard) did not complete. As of the
fix (and re-confirmed 2026-06-20) the full deferred suite completes and
`BENCH_OK` prints. Normal operation was never affected: the default `BOOT_OK`
boot test always passed (the deferred bench suite runs only after BOOT_OK).
