### B-SCHED-SPAWN-DEADLOCK. Intermittent boot hang — boot task spins on `SCHED.lock()` in `spawn_inner` during the `tcc` project-header spawn self-test; SCHED held by an unidentified non-running context — INSTRUMENTED, WATCH 2026-07-15

**Symptom.** A live boot wedge caught by `scripts/wedge-soak.sh`
(`build/hang-catches/soak-20260715-180829-iter15.*`), ~1 in 15 boots.
Distinct from both B-WAITQ-IDLEPARK (a lost-wakeup, IF *set*, HLT) and
B-SYSCTL-IRQ-DEADLOCK (sysctl lock). The i6300esb NMI hard-lockup watchdog
captured CPU#0 (the only CPU) at `RIP=0xffffffff815c8976` =
`core::sync::atomic::spin_loop_hint`, `RFL=0x00000002` (**IF cleared** →
spinning with interrupts disabled = a spinlock deadlock, not a lost-wakeup).
`RDI=0xffffffff8271efb0`, which `llvm-nm` resolves to **exactly**
`kernel::sched::SCHED` — so the CPU is spinning on `SCHED.lock()`.

**Where it hangs.** `llvm-objdump -dl` on the frozen RIP + backtrace
resolves the stack (fresh `target/x86_64-unknown-none/debug/kernel`,
Jul 15 18:06) to:
`kernel_main → spawn::self_test_linux_real_glibc_cc_project_header →
spawn_reap_tcc → proc::spawn::spawn_process → spawn_process_inner →
proc::thread::spawn → sched::spawn_suspended → sched::spawn_inner →
cpu::without_interrupts{closure} → spin_loop_hint`. The spin is the inlined
`spin::Mutex::lock` of `SCHED.lock()` at `sched/mod.rs` ~1189 (address
operand `0x8271efb0` in the disasm == `&SCHED`), inside the
`without_interrupts` critical section. Serial ends mid-`[spawn] Running REAL
project-header C build (tcc, #include "...", ring 3, Path Z) test…`
(process 217), i.e. the *6th* `tcc` spawn (the prior 5 tcc self-tests all
passed) — so it is timing/allocation-pattern dependent, not a static
double-lock. The post-BOOT_OK liveness watchdog fired (heartbeat=4328,
ctx_switches=1122) and confirmed `!! could not acquire SCHED lock — a task
is likely wedged holding it`.

**Root cause — NOT yet pinned.** On a single CPU, SCHED can only be *held
while its holder is not running* if some context acquired SCHED and then a
context switch occurred before it released — yet: (a) involuntary preemption
is already deferred while SCHED is held (`do_deferred_preempt` checks
`SCHED.is_locked()`, sched/mod.rs ~2666), and (b) a *voluntary* yield while
holding SCHED would immediately self-deadlock in `schedule_inner`'s own
`SCHED.lock()` (which would show `schedule_inner` in the backtrace, not
`spawn_inner`). Static analysis this session could not identify which of the
~50 `SCHED.lock()`/`try_lock()` sites leaks it, nor the exact race
(candidate: a `SCHED.is_locked()`-guard race, or a fault/exception while
holding SCHED). SCHED is a raw `spin::Mutex` and does **not** bump
`preempt_count` (only `crate::sync::Mutex` does), so the `spawn_inner`
voluntary-switch guard at sched/mod.rs ~4744 — which checks `preempt_count`
— would *not* catch a SCHED-held voluntary switch.

**Diagnostic fix (this session).** Wrapped SCHED in a `SchedMutex` newtype
(sched/mod.rs) mirroring the existing `mm::heap` HEAP_LOCK_OWNER/SITE
mechanism: `lock`/`try_lock` are `#[track_caller]` and record the holder's
task-id + CPU + `&'static Location` acquire site into
`SCHED_LOCK_{OWNER,CPU,SITE}` atomics; the `SchedGuard` clears them on drop.
`try_lock` records **only on success** so a failing probe never clobbers the
real holder's record. New lock-free `dump_sched_lock_owner()` prints
`tid=… (cpu …) acquired at file:line:col` and is now called from the
liveness "could not acquire SCHED lock" path (~2273). The `SchedGuard`
derefs transparently to `SchedState`, so all existing call sites are
unchanged. This turns the "victim spinning in spin_loop_hint" capture into a
direct pointer at the leaking acquire site on the next reproduction.

**2nd catch (iter21, 2026-07-15 run 204740).** Re-caught at `RIP=spin_loop_hint`,
`RDI=&SCHED` again — but `RFL=0x202` (**IF *set*** — interrupts enabled, distinct
from iter15's IF-cleared `without_interrupts` spin) during the same tcc ring-3
spawn self-test (`[spawn] Running REAL C compiler (tcc, ring 3, Path Z) test`).
20 clean boots preceded it. The new SCHED holder-tracking dump fired and
reported: `SCHED-lock: record shows unlocked … but the lock is still physically
held`. **Key inference:** on a single CPU, `record()` runs immediately after the
physical acquire, so `OWNER == u64::MAX` while the lock is physically held means
the holder is wedged in the tiny window *between* `self.0.lock()` and `record()`
— which only stalls for 15+ s if a **fault or interrupt lands in that window and
re-enters `SCHED.lock()` on the same CPU** (a single-CPU self-deadlock: spinning
to acquire a lock whose holder can't run). The page-fault handler
(`idt.rs::handle_page_fault`) does `cpu::sti()` when the faulting context had
IF=1 and then runs long fault resolution (demand paging, CoW, swap) — a plausible
source of same-CPU re-entrancy. `account_fault` already uses `try_lock` (safe);
the offending re-entrant `SCHED.lock()` site is not yet pinned.

**2nd diagnostic pass (this session).** The original holder-tracking could not
name the holder in the iter21 case (OWNER was never written — the holder was in
the acquire→record window). Added a **per-CPU acquire-site stack**
(`SCHED_ACQ_SITES`/`SCHED_ACQ_DEPTH`, sched/mod.rs): `SchedMutex::lock`/`try_lock`
push `Location::caller()` **before** taking the physical lock and pop on guard
drop. This captures (a) a holder wedged in the acquire→record window (its site is
already pushed) and (b) the full **nesting chain** when a fault/IRQ handler
re-enters SCHED on the same CPU — naming BOTH the outer holder and the inner
deadlocking acquirer, which the NMI frozen-RIP capture (always just
`spin_loop_hint`) and the OWNER record alone cannot. `dump_sched_lock_owner` now
always prints every CPU's acquire-stack (`SCHED acquire-stack cpuN: depth=… →
[lvl] file:line:col`).

**3rd soak (2026-07-15 run 215126, 60 iters, instrumented).** Ran the full
acquire-stack build for 60 consecutive boots: `WEDGE_SOAK_DONE rc_caught=0` — the
race did **not** fire this run (all 60 BOOT_OK, 93–113 s each). Consistent with
the measured ~1-in-20-to-28 recurrence: 60 clean boots is within ordinary bad
luck. No new artifact produced. Rather than keep re-running clean soaks (a
no-edit verification loop), the diagnostic net is now **permanently baked into
the kernel** (ba717f518), so the acquire-stack dump will fire automatically on
the *next* wedge in any routine boot-test or soak — no dedicated hunt needed.

**Next step.** WATCH — the instrumentation is in place. On the next re-catch
(regs newer than `soak-20260715-204740-iter21`), read the
`[liveness]   SCHED acquire-stack cpuN:` frames — the deepest (inner) frame is
the re-entrant `SCHED.lock()` that deadlocks, the outer frame is the holder.
Then fix the re-entrancy (make the inner site non-blocking `try_lock`, or ensure
SCHED is not held across a fault-prone / interruptible region).

**LIKELY ROOT CAUSE FOUND (2026-07-15, static audit) — see
B-COMPLETION-TIMER-IRQ-DEADLOCK below.** A static audit of *every* IRQ/softirq
path that can reach a blocking `SCHED.lock()` found exactly one such site: the
timer softirq's `ipc::timer::process_timer_expirations` → `completion::notify`
→ blocking `sched::wake()`. A timer softirq runs with interrupts enabled and can
preempt a task holding `SCHED` (holders don't disable interrupts), so if a
completion-port timer happens to expire in that window the softirq's
`sched::wake()` re-enters `SCHED.lock()` on the same CPU and spins forever —
**exactly** this bug's signature (RIP=`spin_loop_hint`, RDI=`&SCHED`, IF set,
"record shows unlocked" because the holder was mid acquire→record when the timer
fired). The tcc ring-3 spawn just widens the window (lots of SCHED traffic). This
is the same interrupt-reentrancy class as B-SYSCTL-IRQ-DEADLOCK. **Fixed** in the
same pass (softirq-safe `completion::try_notify` + retry-on-contention in
`process_timer_expirations`). Keep the acquire-stack instrumentation and the WATCH
status until a long soak confirms the wedge no longer reproduces; if it *does*
still fire after this fix, the acquire-stack dump will name the true site.
