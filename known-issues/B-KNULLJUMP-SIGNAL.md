### B-KNULLJUMP-SIGNAL. Rare kernel jump to `RIP=0x0` during the tcc-signal Path-Z self-test, cascading into a kernel-stack-overflow #DF/#UD storm — DIAGNOSTICS HARDENED, ROOT-CAUSE OPEN, WATCH 2026-07-16

**Class.** A control-flow hijack in kernel context: the kernel executed a
`call`/`jmp` through a **null (or corrupted) code pointer**, landing at
`RIP=0x0`. This is *not* a spinlock/deadlock bug (distinct from
B-COMPLETION-TIMER-IRQ-DEADLOCK, whose fix held across all 40 soak iterations).

**Symptom / evidence.** Caught once by `scripts/wedge-soak.sh` at
`build/hang-catches/soak-20260715-235730-iter40.{serial,regs}.txt` — 1 catch in
40 boots (the tcc-signal test itself *passed cleanly* in iters 1/5/20/39, so this
is rare and intermittent, not deterministic). Serial trace (iter40, line ~3262):

```
[spawn] Running REAL C compiler (tcc, HOSTED glibc link, signal, ring 3, Path Z) test...
EXCEPTION: Page Fault (#PF) at 0x0, address=0x0, error=0x10   <- kernel I-fetch @ 0x0
  Cause: not-present, read, kernel
  CS=0x8 RFLAGS=0x10646 RSP=0xffffc100000270a0 SS=0x10
EXCEPTION: Page Fault (#PF) at 0xffffffff815544fe, address=0x3c8fcd6c4d, error=0x0  <- diag path faults
...  (recursive #PF storm, RIP==CR2 descending the kstack ~0x850/frame, error=0x11 NX)
EXCEPTION: Double Fault (#DF) at 0xffffffff80fd2141
  RSP 0xffffc10000017ff8 is in a kstack GUARD PAGE — KERNEL STACK OVERFLOW confirmed
EXCEPTION: Invalid Opcode (#UD) ...  (garbage-execution storm in .data) -> NMI watchdog
```

The crash fires **immediately after** the "signal, ring 3, Path Z" banner and
**before** the `[spawn] ELF validated` line that a clean run prints next — i.e.
during kernel-side setup of the tcc-signal spawn, right after the *previous*
test's task exited (`Process 222 ... now zombie` / `Task 188 exiting`). Prime
suspicion: a use-after-free / teardown race on a kernel code pointer (a
completion/workqueue/timer callback, or a saved return address) at the
zombie-cleanup → next-spawn boundary. On UP the only concurrency is
interrupt/softirq preemption, so a softirq firing mid-setup and invoking a
freed/zeroed callback is a plausible mechanism. Not yet pinned.

**Secondary bug (FIXED this session).** The single null-jump was *buried* under a
4400-line cascade because the fatal kernel-`#PF` path ran stack-hungry
diagnostics (RBP frame-walk + formatting) with **no re-entrancy guard**: when
those diagnostics themselves faulted (the second `#PF` at `0xffffffff815544fe`),
the nested `#PF` restarted the whole report, recursing ~2 KiB/level until the
kstack overflowed into `#DF` → `#UD` storm → NMI watchdog. Fix in
`kernel/src/idt.rs`:
- Added `FATAL_FAULT_IN_PROGRESS: AtomicBool`. `handle_page_fault` checks it at
  entry (before `sti`/resolve) and, if set, prints one minimal line and halts —
  no recursion. It is armed at the start of the fatal kernel-`#PF` branch, before
  any diagnostics. `handle_double_fault` swap-arms + checks it too.
- Reordered the fatal-`#PF` diagnostics so the **safe** raw stack-scan
  (`dump_stack_scan`, which validates every slot against known kstack regions
  before dereferencing) runs *before* the `print_current` RBP walk (which blindly
  follows a possibly-corrupted chain and can fault). This guarantees the
  return-address candidates that name the hijacked caller reach serial even if
  the walk trips the guard and halts.

Net effect: the next time this null-jump reproduces, we get a single clean fault
report **plus a stack-scan naming the caller of the null pointer**, instead of a
stack-overflow storm that destroys the evidence.

**Where it lives.** Bug: unknown (tcc-signal spawn setup / zombie-cleanup race).
Diagnostics fix: `kernel/src/idt.rs` (`handle_page_fault`, `handle_double_fault`,
`FATAL_FAULT_IN_PROGRESS`). Detector: `scripts/wedge-soak.sh`.

**Next step.** Re-run `wedge-soak.sh` (long, 60+ iters) to re-catch with the new
diagnostics and read off the caller from the stack-scan; that pins the corruption
site so a proper root-cause fix can follow. Until then this is WATCH — rare,
and the cascade (the part that took down the whole machine hard) is now contained
to a clean halt.

**UPDATE 2026-07-16 — reproduction-hunt soak came back clean.** Ran an 80-iter
`wedge-soak.sh` (`soak-20260716-014354`) specifically to re-catch this with the
new diagnostics: **0 catches in 80 boots** (all passed, e.g. iter80 BOOT_OK
121s). Combined with the original 1/40, the empirical rate is ~1-in-120 — too
rare to force in a bounded soak. Ending the *dedicated* hunt (per the
no-idle-loop rule: a clean verification loop is done). This stays WATCH: the
diagnostics fix (commit 6fb1597aa) is permanently in place, so the **next**
spontaneous occurrence — in any future soak or a normal boot — will self-report a
single clean fault line plus a stack-scan naming the null pointer's caller, which
is what's needed to pin and fix the root cause. No further action until then.

**UPDATE 2026-07-16 — deferred-callback dispatch paths hardened (defense in
depth).** Audited every kernel path that invokes a *stored code pointer* from
interrupt/softirq/exit context — the mechanism class that would produce this
bug's exact signature (an async `call` through a corrupted/zeroed field →
`RIP=0` or a wild address in kernel context). Findings + fixes:
- **`hrtimer::process_expired`** (`kernel/src/hrtimer.rs`) was the *only* path
  that called a code pointer **directly from the APIC timer ISR** with **no
  validation** (`cb(arg)`), so a corrupted per-CPU `TimerEntry.callback` field
  would jump the ISR straight to a bad address. Now validates the callback
  against real `.text` bounds before dispatch; a rejected pointer is logged
  (`[hrtimer] CRITICAL: refusing to dispatch corrupt timer callback …`) and
  skipped.
- **`ktimer::process_expirations`** (`kernel/src/ktimer.rs`) only rejected an
  *exactly-zero* `func`; strengthened to a full `.text` check so a non-zero-but-
  wild value (torn store / heap overrun) is caught, the slot freed, and logged,
  instead of being submitted to the workqueue and later jumped-to by the worker.
- **`notify_exit_hooks`** (`kernel/src/sched/mod.rs`) only rejected exactly-zero
  hook slots; strengthened to a full `.text` check. This runs **at task-exit
  time — the exact moment this bug fires** (right after "Task N exiting"), so a
  clobbered hook slot now logs-and-skips rather than jumping the dying task's
  context to a wild address.
- **`workqueue::worker_entry`** (`kernel/src/workqueue.rs`) called `(work.func)`
  directly with no validation. This is the single chokepoint where *every*
  submitted callback is finally invoked, so validating here covers all
  submitters at once; a rejected entry is logged and dropped.
- **`rcu::process_callbacks`** (`kernel/src/rcu.rs`) dispatches deferred
  callbacks from the BSP softirq (`rcu::tick`) via `(cb.func)(cb.arg)` with no
  validation; now `.text`-checked, logged + skipped on failure.
- Exposed `idt::is_kernel_text` as `pub(crate)` (precise linker-symbol
  `__text_start..__text_end` bounds) as the shared validator.

With these, **all five** kernel deferred-code-pointer dispatch sites (hrtimer,
ktimer, exit-hooks, workqueue, rcu) now validate against `.text` before calling
— whichever one is the corruption victim, the next occurrence self-reports which
subsystem and what `arg` was involved instead of jumping to `RIP=0`.

**Follow-up 2026-07-16 — audit completed to the last two indirect-call sites.** A
full kernel sweep for stored/transmuted `fn`-pointer dispatch (`transmute`-to-`fn`
and `.<field>)(…)` indirect calls) confirmed only two more registration-table
call sites existed beyond the five async ones: `fs::fileinfo` custom metadata
extractors (`(ext.func)(…)`) and `mm::pressure` shrinker callbacks
(`(shrinker.callback)(…)`). Both run in *synchronous* thread context (not from a
timer ISR, so they don't match this bug's async-jump-during-spawn signature as
closely), but each is a `Mutex<Vec<struct{fn ptr}>>` whose heap backing could be
clobbered by the same suspected overrun, so both were hardened with the identical
`.text` guard (`[fileinfo]`/`[pressure] CRITICAL: refusing … (see
B-KNULLJUMP-SIGNAL)`). The two `transmute::<u64, fn>` sites (ktimer, exit-hooks)
are the ones already guarded above. **The kernel now validates every
stored-code-pointer dispatch (7 sites total) before calling.** Boot-validated
(BOOT_OK 138s, no false positives).
These are **not** the root-cause fix (the corruption *source* is still unknown),
but they (a) are the correct defensive posture for dispatching a stored code
pointer, and (b) convert the catastrophic wild-jump into a **named diagnostic
that identifies which subsystem carried the bad pointer** — a large step toward
pinning the corruption site on the next occurrence, complementing the idt.rs
re-entrancy guard (6fb1597aa). Boot-validated: BOOT_OK 104s, no false-positive
`CRITICAL` logs (all legitimate callbacks validate as `.text`), hrtimer/ktimer
self-tests still pass. Still WATCH for the underlying corruption.

**Fresh spontaneous occurrence 2026-07-22 — first catch in a *normal boot test*
(not a soak), and on a *different* Path-Z rung.** During routine boot-test
validation of an unrelated change (adding the `fastpy-wc` ring-3 self-test), the
null-jump fired during the **`REAL glibc dynamic-execution (ring 3, Path Z)`**
test — *not* the tcc-signal rung of the original catch. Serial (build/serial-test.txt):
```
[spawn] Running REAL glibc dynamic-execution (ring 3, Path Z) test...
EXCEPTION: Page Fault (#PF) at 0x0, address=0x0, error=0x10   <- kernel I-fetch @ 0x0
  Cause: not-present, read, kernel
  CS=0x8 RFLAGS=0x10646 RSP=0xffffc100000270a0 SS=0x10
NESTED #PF during fatal diagnostics: rip=0xffffffff815da4de cr2=0x2fccaee1d6 err=0x0 — halting.
```
Confirms this bug's exact signature (`#PF at 0x0, addr=0x0, error=0x10`, kernel
instruction-fetch through a null code pointer, same `RFLAGS=0x10646` /
`RSP=0xffffc100000270a0` as iter40) and that it is **not** tcc-signal-specific —
it strikes at *any* Path-Z spawn/teardown boundary, supporting the
zombie-cleanup→next-spawn-race hypothesis over any per-test cause. This
reproduction is **unrelated to the `fastpy-wc` change** (the wc + grep self-tests
both ran and passed cleanly far earlier — wc printed `4 6 30` and exited 0; this
is a *later* test). A subsequent boot-test re-run passed clean to BOOT_OK,
consistent with the ~1-in-120 intermittency.

- **Good:** the idt.rs re-entrancy guard (6fb1597aa) worked — a single clean halt
  line, **no** #DF/#UD stack-overflow storm (contrast iter40's 4400-line cascade).
- **Gap exposed — and its true cause pinned + FIXED this session.** The nested
  `#PF` fired at `rip=0xffffffff815da4de` *before* the safe `dump_stack_scan` could
  print, so we **again got no stack-scan naming the null pointer's caller.**
  `llvm-nm` on the crash binary maps `0xffffffff815da4de` to
  `alloc::collections::btree::node::slice_insert::<kernel::fs::history::FileHistory>`
  — a **BTree insert, not the fatal-diagnostic code at all.** That is the tell: the
  nested fault was **not** a diagnostic self-fault, it was an *interrupt-context*
  fault racing the fatal report. Root cause: `handle_page_fault` re-enables
  interrupts (`sti()`) early to make paging preemptible when the faulting context
  had `RFLAGS.IF=1` (this fault: `RFLAGS=0x10646`, IF set), so the entire
  stack-hungry fatal-diagnostic path ran **interruptible** — a timer tick / softirq
  / deferred fs-history BTree insert fired mid-report, itself faulted (plausibly on
  the same heap corruption), re-entered `handle_page_fault`, and tripped the
  re-entrancy guard, halting us before the scan printed. **Fix (idt.rs
  `handle_page_fault`, this session):** (1) `cpu::cli()` as the first action of the
  unrecoverable-kernel-fault branch (before arming `FATAL_FAULT_IN_PROGRESS`) — the
  fatal path ends in `halt_loop()` regardless, so making it atomic costs nothing and
  stops any interrupt-context nested fault from racing/burying the report; (2) moved
  `dump_stack_scan(frame.rsp, 64)` to run **before** the `sched::panic_diagnostics()`
  task-name lookup (a locked deref of scheduler state that can itself fault when
  that state is the corruption victim), so even a diagnostic self-fault can no longer
  pre-empt the scan. Net: the **next** B-KNULLJUMP occurrence should finally emit the
  stack-scan naming the null pointer's caller. Boot-validated (BOOT_OK, no
  false-positive fatal reports). Still WATCH for the underlying corruption source.

**BREAKTHROUGH 2026-07-22 — first symbolized capture + a RELIABLE (layout-locked)
reproducer; victim pinned to a scheduler `BTreeMap` iteration.** While finishing
the unrelated `fastpy-ls` tool (commit 3f10b31f2), removing a temporary
kernel-side `Vfs::readdir` diagnostic from `self_test_fastpy_slateos_ls` shifted
kernel/heap layout into a configuration where this corruption reproduces **2/2
boots at the *identical* fault site** — the first non-intermittent reproducer of
a bug that had been ~1-in-120. The idt.rs hardening from earlier this session
finally paid off: the full stack-scan printed *before* the nested fault, so we
have real symbols for the first time.

Serial (build/serial-test.txt, during `REAL glibc dynamic-execution (ring 3,
Path Z)`, process 199, right after a demand-page `[mmap] Lazy mapped …`):
```
EXCEPTION: Page Fault (#PF) at 0xffffffff816618de, address=0xbb, error=0x0
  Cause: not-present, read, kernel        <- DATA read, not an I-fetch
  CS=0x8 RFLAGS=0x10446 RSP=0xffffc100000277f0
  Task: 0 ("")                            <- faulted on the IDLE task
NESTED #PF during fatal diagnostics: rip==cr2==0xffffc10000026cf0 err=0x11 (I-fetch into stack)
```
Symbolized against `target/x86_64-unknown-none/debug/kernel` with `llvm-nm
--print-size --numeric-sort` (nearest-preceding-symbol):
- **Fault rip `0xffffffff816618de` = `alloc::collections::btree::navigate::…::next_kv +0x5e`**
  — i.e. **BTreeMap *iteration*** dereferencing a corrupted leaf/edge node pointer
  (reads data at `0xbb`, a near-null offset off a ~null base).
- Stack-scan return-address candidates (a scan, not a verified chain, but the
  cluster is telling): `btree::search::find_key_index +0x22a`,
  `btree::navigate::next_unchecked` (+ its closure), plus scheduler frames
  `kernel::sched::load_current_task`, `kernel::sched::SchedMutex::record`, and the
  bss globals `CURRENT_TASK_IDS` / `SMP_INITIALIZED`. (`load_current_task` merely
  reads an atomic — it's a stale stack value, not the real caller — but the btree
  + sched clustering places the corrupt tree in the scheduler.)

**Refined diagnosis.** This is a **data-read** variant (`addr=0xbb, error=0x0`)
of the same Path-Z teardown corruption, distinct from the classic **code-fetch**
variant (`#PF at 0x0, error=0x10`, wild jump through a null fn-pointer): same
root (something scribbles a freed/heap object at a spawn/teardown boundary), but
this time the victim is a **scheduler `BTreeMap` node** (almost certainly
`SchedState.tasks: BTreeMap<TaskId, Task>`, iterated at many sites in
`sched/mod.rs` — 968/2567/3015/3157/4373/4446/4773 — several reachable from the
idle-task migration/anti-starvation path) rather than a stored code pointer. The
nested fault (`rip==cr2`, I-fetch into the kernel stack, `err=0x11`) is
`sched::panic_diagnostics()` returning through a corrupted stack return address —
the corruption reaches the stack too.

**Why iterator-invalidation is *not* the likely cause:** every `state.tasks`
iteration runs under `SchedMutex` (a `Mutex<SchedState>`), and safe Rust forbids
concurrent mutation within one CPU while the same lock serializes across CPUs —
so this is genuine **adjacent-overflow / use-after-free** corruption of the BTree
node's heap allocation, not a mid-iteration mutation.

**Recommended next step (durable, layout-independent):** add **heap-allocator
corruption detection** — per-allocation redzones + a poison-on-free / short
quarantine in the kernel allocator (`mm/heap` / linked-list or slab) with a
validate-on-alloc/free check that *fires at the corruptor's write*, not later at
the victim's read. That catches the bug regardless of layout (the current
reliable reproducer is layout-fragile and will dissolve on the next kernel edit,
so don't rely on it). The Path-Z teardown sequence (process 199 glibc
zombie-cleanup → idle-task migration) is the window to instrument. Deferred here
because it's a sizable allocator change on top of an already-large fastpy-ls
task; captured now so a focused session can act on the concrete symbols above.
Still WATCH.

**UPDATE 2026-07-23 — KASAN-style shadow memory built (Q32→A, operator-approved;
§86).** Landed a real KASAN shadow-memory subsystem (`kernel/src/mm/kasan.rs`)
as the durable, layout-independent tooling the step above called for:
- **1:8 shadow** over the HHDM heap (`shadow(addr) = KASAN_SHADOW_BASE +
  ((addr - hhdm) >> 3)`), reserved at `0xFFFF_E000_0000_0000` (512 MiB → covers
  4 GiB heap; registered in `kvspace.rs::KASAN_SHADOW`), **lazily mapped** per
  16 KiB shadow frame on first touch (bitmap-gated; mapping is heap-lock-free —
  frame alloc + `map_frame`, guarded by `MAP_LOCK` with IRQs off).
- **Generic-KASAN encoding**: `0x00` addressable, `0x01..07` partial trailing
  granule, `0xFA` freed (UAF), `0xFB` redzone. Fail-open for untracked/unmapped
  shadow (no false positives).
- **Hooked into the allocator** (`mm/heap.rs` `alloc`/`dealloc`, both the
  per-CPU and global paths): `on_alloc` marks the object addressable + poisons
  the slot's trailing redzone; `on_free` poisons the whole slot as freed.
  Runtime-gated by `kasan::is_enabled()` (default **off** — one relaxed atomic
  load per alloc/free, protecting the <200 ns target), so a normal boot is
  unaffected.
- **`kasan::check_access(addr, size, is_write)`** is the checked-store/load
  **shim** for the suspect paths — the substitute for compiler store
  instrumentation the operator's decision anticipated.
- Boot self-test (`mm::kasan::self_test`) exercises the whole lazy-map + poison
  + check pipeline with **real heap allocations** (40-in-64 redzone, freed-slot
  UAF, 12-in-16 partial granule, out-of-range fail-open).

**Remaining to actually catch B-KNULLJUMP** (next step): (1) place
`kasan::check_access` calls on the scheduler `BTreeMap` node access / Path-Z
teardown paths (`sched/mod.rs` `state.tasks` iteration sites; the process-199
glibc zombie-cleanup → idle-task migration window), and (2) enable KASAN around
the Path-Z boot self-tests (`kasan::enable()` before, `disable()` after) so a
checked access to a freed/redzoned node is flagged **at the access**. Landing
the infra first (this update) keeps that follow-up small and low-risk. Still
WATCH.

**UPDATE 2026-07-23 (b) — free-quarantine added + compiler-KASAN feasibility
resolved.** Two follow-ons to the shadow work above, both boot-green:

- **Slab free-quarantine** (`kernel/src/mm/quarantine.rs`). The KASAN shadow
  above is *passive* — it flags a freed/redzoned region only when instrumented
  code *reads/checks* it, so it cannot catch B-KNULLJUMP's actual failure mode:
  a stale-pointer/UAF **write** into a slot that the slab *immediately reused*
  for a live scheduler BTree node (nothing ever checks that write). Quarantine
  attacks that class directly: when enabled, a freed slab slot is filled with
  `POISON_FREE` (0xDE) and parked in a fixed 8192-entry FIFO ring instead of
  being handed straight back out. While parked, the live node can **never be
  allocated at that address** (so if the corruption vanishes under quarantine,
  it's confirmed a reuse/UAF — hypothesis 1 — not an in-place overflow), and a
  lingering stale write lands on parked poison, caught on eviction (`on_free`)
  or an on-demand full-ring sweep (`scan_all`) with address/class/offset
  reported. Runtime-gated (`quarantine::is_enabled()`, default **off** — one
  relaxed load per free); when on, frees route through a global locked ring and
  bypass the per-CPU fast path (perf irrelevant during a hunt). `drain()`
  reclaims parked slots afterward. Self-test (`mm::quarantine::self_test`)
  exercises park/poison/FIFO-eviction/`scan_all`/`drain`; verified in-boot it
  flags a stomped parked slot ("byte +7 = 0x00 … B-KNULLJUMP candidate").

- **Compiler-level KASAN IS available** on our kernel target. Probing
  `rustc +nightly --target x86_64-unknown-none --print target-spec-json`
  shows `supported-sanitizers: ['kcfi', 'kernel-address']` — i.e. LLVM
  `-Zsanitizer=kernel-address` (which auto-instruments *every* load/store,
  catching arbitrary wild writes with no manual shim) compiles for this target.
  This is the **definitive** tool to catch B-KNULLJUMP's exact faulting store.
  It was NOT taken now because it is a large, higher-risk bring-up and a genuine
  build fork: it needs (a) shadow backing for the **entire** accessed kernel VA
  (text/data/stacks/HHDM/MMIO), not just the heap — Linux maps a shared
  read-only zero shadow page for untracked regions; (b) the `__asan_*` /
  `__kasan_*` runtime callback symbols defined in-kernel; (c) a fixed
  compile-time shadow offset (`-Cllvm-args=-asan-mapping-offset=…`,
  `-asan-mapping-scale=3`) matching our layout, viable since HHDM is
  deterministic (0xFFFF_8000_0000_0000); (d) `#[no_sanitize]` on all
  shadow-setup / early-boot paths and correct bring-up ordering; and (e) most
  likely a separate debug build profile (whole-kernel instrumentation is a big
  perf hit). Sequencing the lighter, lower-risk shadow + quarantine tools first
  (this + the (a) update) is the right order; escalating to a full
  compiler-instrumented KASAN kernel is the fallback if they don't localize it.
  Flagged as a design fork for the operator — see `open-questions.md` (Q34).

**SIGHTING 2026-09-04 (lane A, commit be4600d6a) — the code-fetch variant landed
on a MAPPED page, so it raised `#UD` and took a handler with none of the
hardening. Handler gap FIXED this session; corruption still open.**

Caught by an ordinary boot test (not a soak): `PANIC` at 115s, recorded in
`bench/boot-history.jsonl`. Serial preserved at
`build/hang-catches/knulljump-20260904-be4600d6a.serial.txt`.

```
[mmap] Unmapped 1 frames at 0x600003c000..0x6000040000
[thread] Process 218 has no threads left — now zombie
[sched] Task 180 exiting
[mmap] Committed mapped 0x600008c000..0x6000090000 (1 frames)
[mmap] Committed mapped 0x6000090000..0x6000094000 (1 frames)
EXCEPTION: Invalid Opcode (#UD) at 0xffffffff82a8f5b0
  CS=0x8 RFLAGS=0x10282 RSP=0xffffc100005630b8
  Instruction bytes: ff ff 00 00 01 00 00 00 d8 5b 83 82 ff ff ff ff
  Task: 181 ("fastpy-countin."), cpu 0
  Backtrace (2 frames):
    # 0: 0xffffffff80a5d6c2      <- handle_invalid_opcode+0x3a2
    # 1: 0xffffffff80a4f252      <- isr_invalid_opcode+0x2c
FATAL: Unrecoverable kernel #UD. Halting.
```

**Same bug, same boundary.** The trigger signature matches this entry exactly:
the fault fires immediately after the previous test's task exited
(`Process 218 … now zombie` / `Task 180 exiting`), at the
zombie-cleanup→next-spawn boundary, in the *next* task — here task 181 running
the freshly-`exec`'d fastpy `countin`, two demand-page commits into its new
image. It is a Path-Z fastpy spawn, consistent with "not tcc-signal-specific …
it strikes at *any* Path-Z spawn/teardown boundary".

**What is new: the landing address was not null, and that is informative.**
`scripts/symbolize.py` resolves `0xffffffff82a8f5b0` to
`kernel::ktrace::CATEGORY_MASK [d]` — a live `AtomicU32` in `.data`, not a
freed or zeroed cell. The dumped bytes confirm the symbolization independently:
`CATEGORY_MASK` is declared `AtomicU32::new(0xFFFF)` (ktrace.rs:281) and the
first four bytes read `ff ff 00 00`. So the hijacked pointer held a **plausible,
correctly-formed kernel address that simply was not a function** — the following
qword `d8 5b 83 82 ff ff ff ff` is itself another kernel pointer, i.e. RIP had
branched into a *table of pointers*.

That distinguishes this capture from the 2026-07-15 one (`RIP=0x0`). A null RIP
is consistent with "the object was freed and zeroed"; a valid-looking `.data`
address is not. Whatever scribbles the victim is writing **real pointer values**,
which points at a type-confused or off-by-N read of a live structure — reading a
code pointer from the wrong offset — rather than at reading cleared memory.

**Why this capture is nearly useless for root-causing, and why that is now
fixed.** Whether a wild jump reports `#PF` or `#UD` is decided by nothing more
interesting than whether the garbage address happens to be mapped. `RIP=0x0` is
unmapped → `#PF`, which is the handler that received three rounds of hardening
(re-entrancy guard, early `cli()`, stack-scan-before-`panic_diagnostics`).
`&CATEGORY_MASK` is mapped → `#UD`, and `handle_invalid_opcode` had **none** of
it: no `cli()`, no `FATAL_FAULT_IN_PROGRESS`, no `dump_stack_scan` at all, and
`sched::panic_diagnostics()` (a locked deref of the scheduler `BTreeMap` — this
bug's own pinned victim) running *before* any scan. The prediction recorded
above, that "the **next** B-KNULLJUMP occurrence should finally emit the
stack-scan naming the null pointer's caller," did not hold: the occurrence came
through the sibling handler, and all we got was a two-frame RBP walk of the
fault handler's own frames.

**Fix (this session, `kernel/src/idt.rs` `handle_invalid_opcode`):** mirrored the
fatal-`#PF` branch into the kernel-`#UD` branch — `cpu::cli()` first, then
`FATAL_FAULT_IN_PROGRESS.swap` with a `NESTED #UD` one-liner, then
`dump_stack_scan(frame.rsp, 64)` *before* the instruction-byte dump,
`panic_diagnostics()` and `print_current()`. Placed after the ring-3 early
return, since a userspace `#UD` is routine (CFI traps, the deliberate
compiler-trap self-test) and must not arm a fatal guard or disable interrupts.
The next occurrence on either path should now name the caller.

**Not a regression from this batch.** The tree under test contained no scheduler,
mm, proc or exec change from lane A; the batch was boot-test instrumentation,
script gates, and lane C application work. The bug predates it by seven weeks.
