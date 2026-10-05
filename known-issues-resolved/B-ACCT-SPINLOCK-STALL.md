### B-ACCT-SPINLOCK-STALL. `ACCT` (mm memory-accounting) spinlock self-deadlock — ROOT-CAUSED + FIXED 2026-07-03

**STATUS: FIXED** (commit this session). Root cause confirmed by the
owner-tracking instrumentation: a **recursive self-deadlock** — the same task
that holds `ACCT` re-enters it from interrupt context. Fix: acquire `ACCT` via
the new `Mutex::lock_irqsave()` (interrupts masked for the hold), the standard
`spin_lock_irqsave` discipline for a lock shared with interrupt context. See
"Root cause + fix" below. Re-soak to confirm no recurrence.

**Root cause + fix (2026-07-03):** The instrumented soak reproduced on
**iteration 1** and the owner stamp printed the verdict verbatim:
`[sync]   lock 'ACCT' holder: task 138 == spinner — RECURSIVE self-deadlock
(same task re-entered the lock)` (task 138 = "countbytes", the ring-3
`/bin/emit | /bin/countbytes > file` pipeline; catch:
`build/hang-catches/ACCT-OWNER-recursive-task138.txt`).

Mechanism (uniprocessor — no cross-CPU AB-BA needed):
1. `Mutex::lock()` disables *preemption* but **not interrupts** — it leaves IF
   as-is. `ACCT` was acquired this way.
2. `ACCT` is reachable from **interrupt/softirq context**: the frame allocator
   calls `compact::try_compact()` for any `order > 0` allocation
   (`mm/frame.rs:2033`), and compaction's `estimate_movable_pages()` calls
   `accounting::tracked_count()` (`mm/compact.rs:266`) → acquires `ACCT`. So a
   device IRQ / softirq that allocates a multi-order buffer re-enters accounting.
3. Critically, the **page-fault handler re-enables interrupts** (`idt.rs:2048`,
   `cpu::sti()` when the faulting context had IF=1) *before* calling
   `mm::fault::resolve` → `map_frame`/CoW → `charge`/`uncharge`. So a
   `charge`/`uncharge` on the fault path runs and holds `ACCT` **with interrupts
   enabled**.
4. An interrupt lands while `ACCT` is held → its handler allocates an
   order>0 frame → compaction → `tracked_count()` → tries to re-acquire `ACCT`
   → spins forever (holder can never resume to release it). On UP the spinner
   *is* the same task's IRQ frame, so `owner == spinner` → the recursive verdict.

Why the earlier static analysis missed it: I looked only for a *direct*
IRQ-context accounting caller and found none; the real path is indirect
(IRQ → frame alloc → compaction → `tracked_count`) and is only opened by the
page-fault handler's `sti`. The accounting functions themselves remain correct
leaf scans; the bug was the *locking discipline*, not the functions.

**Fix:** added `Mutex::lock_irqsave()` + `MutexIrqGuard` to `kernel/src/sync.rs`
(save IF, `cli`, acquire; guard restores IF after releasing the lock and
re-enabling preemption — reverse of acquire order; nests correctly, only the
disabling edge restores). Switched all 12 `ACCOUNTING.lock()` sites in
`kernel/src/mm/accounting.rs` to `lock_irqsave()`. This masks interrupts for the
short leaf-only hold, closing the re-entry window for *any* interrupt (not just
the compaction path). A nested #PF cannot occur during the hold (the functions
only touch a static `.bss` array + trivial stack), so masking maskable
interrupts is both necessary and sufficient. Builds clean, no new clippy
warnings. Module doc in `accounting.rs` updated to document the IRQ-safety
requirement.

**Follow-up (separate, low priority):** `all_stats()` still `.collect()`s a
`Vec` under the lock (now under `lock_irqsave`, so interrupts are masked across
a heap alloc — worse for IRQ latency, though it has no live callers). Should be
count-then-release or a fixed stack buffer regardless.

---

<details><summary>Original investigation notes (pre-fix, kept for history)</summary>

#### B-ACCT-SPINLOCK-STALL. `ACCT` (mm memory-accounting) spinlock stuck at end of ring-3 battery — REPRODUCED 2026-07-03

**Where:** `kernel/src/mm/accounting.rs` (the `ACCOUNTING` spinlock, named `b"ACCT"`,
line 102) / `kernel/src/sync.rs` (the `Mutex` wrapper). Caught by the armed
hang-repro soak on **iteration 7/24** with the orphaned-Running-fixed kernel:
`build/hang-catches/ACCT-STALL-iter7-*.txt`.

**This is a DISTINCT bug from the orphaned-Running dispatch wedge** (which was
committed just before this soak). Decisive discriminator: the catch shows **no
`[sched] BUG:` line**, so the fixed dispatch path is not involved.

**Observed signature (`ACCT-STALL-iter7`):**
- `[liveness] SYSTEM HANG: no task-level forward progress for 15+ seconds
  (useful_work=140, all CPUs idle-ticking)` — cpu0 heartbeat=3501 **still
  advancing** (BSP alive, not an IF=0 spin), `local_has_real_work=false`,
  `last_rip=0xffffffff81107fb9 (kernel_text)`.
- Task dump: **91 tasks, 90 `state=Dead`, only `tid=0` (the boot/self-test
  driver, name overwritten to "prctl-batch269") is `state=Running`** on cpu0 at
  prio=31. This is the very end of the ~34-test ring-3 battery — everything ran
  and exited, leaving only the driver.
- Then: `[sync] *** SPINLOCK STALL *** lock 'ACCT' still not acquired after ~30s
  of spinning (cpu 0, task 0, 66805760 iters). Likely self-deadlock or lock
  convoy` followed by `[lockdep]   cpu 0 holds 0 lock(s):`. So task 0 spins
  ~66M iters trying to acquire `ACCT`, which the timer-driven liveness watchdog
  cannot rescue (the spin holds the CPU with preemption disabled).

**Analysis so far (static; not yet definitive):** The `ACCT` lock is
`mm/accounting.rs`'s `Mutex` (a `spin::Mutex` that does **not** disable
interrupts — `lock()` only `preempt_disable()`s). All *live* callers of the
accounting functions (`charge`/`uncharge` on the map/unmap/CoW page-fault path;
`query`/`tracked_count`/`largest_rss`/`memory_info` from procfs/kshell/
diagnostics/invariant checks) run in **task context** — I could not find any
IRQ/softirq/timer-context caller, which argues *against* a simple
interrupt-reentrancy self-deadlock. The accounting functions themselves are all
leaf scans that never yield/fault/allocate under the lock, so a single call
cannot leak the guard. The one structurally-unsafe function, `all_stats()`
(collects a `Vec` *under* the lock — violates the module's documented "ACCT is a
leaf lock, never held across other lock acquisitions" invariant), has **no live
callers**, so it is not the trigger here (but should be fixed on its own merits:
count-then-release or use a fixed stack buffer). `lockdep cpu 0 holds 0 locks`
is ambiguous — lockdep may only mark a lock *held* after successful acquire, so
a spinner shows 0, and the true holder (if since-dead) leaves no lockdep trace.

**Instrumentation added (commit this session; `sync.rs`) to make it definitive
on the next repro:** every `Mutex` now records the acquiring task id in a new
`owner: AtomicU64` (set in `make_guard`, cleared to `OWNER_NONE`=`u64::MAX` in
`MutexGuard::drop` — one relaxed per-CPU read+store, negligible next to the CAS
and lockdep call already present). `report_stall` now prints the holder and
classifies the stall:
- `owner == spinner tid` → **recursive self-deadlock** (same task re-entered).
- `owner == some other task` → **guard held by another task** (leaked if that
  task is Dead in the dump).
- `owner == OWNER_NONE` → **lost-unlock / flag desync** (spinlock flag set with
  no recorded holder).
This single datum discriminates all three hypotheses. Builds clean. **STILL OPEN
— re-run the armed soak with the instrumented kernel; the next `ACCT` stall will
name its holder and pin the exact leak/recursion path.**

</details>
