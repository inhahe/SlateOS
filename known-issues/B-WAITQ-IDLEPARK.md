### B-WAITQ-IDLEPARK. Intermittent boot hang in the waitqueue `wait_timeout_ns (expired)` self-test — CPU idle-parked with a Blocked task, lost-wakeup family — INSTRUMENTED, WATCH 2026-07-15

**Symptom.** A live boot wedge caught by `scripts/wedge-soak.sh`
(`build/hang-catches/soak-20260715-173612-iter06.*`), ~1 in 20+ boots.
Distinct from the sysctl deadlock below. The QEMU-monitor register
capture shows CPU#0 (the *only* CPU — `info cpus` lists one) at
`RIP=0xffffffff81b2f895 = kernel::cpu::hlt`, `RFL=0x286` (**IF set** —
interrupts *enabled*), `HLT=1`. Two consecutive `info registers` snapshots
show `HLT` toggling `1 → 0` at the same RIP, i.e. the CPU keeps waking on
each ~10 ms timer tick, runs the ISR, finds nothing runnable, and HLTs
again. So this is **not** a spinlock deadlock and **not** a dead timer —
it is a **lost wakeup**: a Blocked task is never re-dispatched.

**Where it hangs.** Serial ends (7094 lines) mid-`[waitqueue] Running
self-test…`, last line `wait_timeout_ns (zero timeout): OK`. The next
subtest — `wait_timeout_ns (expired)` (kernel/src/sched/waitqueue.rs
~522) — is the **first** subtest that actually *blocks* the boot task on a
real hrtimer (`sleep_ns(500_000)` → `block_current`) and expects a timer
wakeup; every prior subtest was "already true" / immediate-timeout and
never parked. The boot task blocks with no other runnable task, enters the
`schedule_inner` **idle-fallback HLT loop** (sched/mod.rs ~4882), and the
500 µs hrtimer wake never re-dispatches it.

**Root cause — NOT yet pinned.** Static analysis (this session) could not
prove the exact race. The wake path *should* be robust: `sleep_ns` arms an
hrtimer whose callback runs `try_wake` (→ `defer_wake` on lock contention),
`block_current`/`wake` serialise on `SCHED.lock` with a `pending_wake`
handshake, and both `schedule_inner` and the idle-fallback loop
`drain_deferred_wakes_locked`. On a single CPU at idle the callback's
`try_lock` should always succeed and find the task `Blocked`. The two
leading hypotheses are (a) the hrtimer entry/callback is somehow lost so
`process_expired` never fires it, or (b) the wake marks the task Ready but
it is orphaned out of every run queue (Ready-but-unqueued), which
`check_starvation` (only rescues Ready→enqueue? verify) may not recover in
this pre-BOOT_OK window. This is the same "all CPUs idle-ticking"
lost-wakeup signature as the B-PTHREAD-YIELDBUDGET family.

**Diagnostic gap fixed.** The boot-scoped liveness watchdog is armed only
at `BOOT_OK`, so a pre-BOOT_OK idle-park hang produces *no* `[liveness]`
dump — only the bare NMI register capture. Added `dump_idle_fallback_wedge`
(sched/mod.rs): the idle-fallback loop now counts consecutive
nothing-runnable HLT-wakes and, after `IDLE_FALLBACK_WEDGE_TICKS` (500 ≈
5 s), dumps **once** (one-shot `IDLE_FALLBACK_WEDGE_DUMPED`): the parked
task's `state`/`pending_wake`/`last_cpu`, `hrtimer::pending_count()` +
`next_expiry_ns`, `local_has_real_work`, occupied deferred-wake slots, and
the full task table. This turns the silent hang into a self-describing dump
so the *next* reproduction pins root cause (Blocked-forever vs.
Ready-orphan vs. lost hrtimer).

**Also fixed (adjacent UB in the same race family).** `schedule_inner`'s
`picked_id == current_id` guard was gated on `&& requeue`. On the block
path (`requeue == false`) a concurrent wake can re-enqueue the current task
so `pick_next_local` returns it; the old guard let that fall through to the
real switch path, which aliases `&mut *old_p` and `&*new_p` to the *same*
`Context`/`FpuState` (UB) and does a pointless save-then-restore. The guard
now handles `picked == current` uniformly regardless of `requeue` (resume
as Running, return). This likely "worked by luck" before (save then restore
the same memory), so it is probably not the observed hang, but it removes
real UB.

**Next step.** Let `wedge-soak.sh` run until the new dump fires, then read
`build/hang-catches/*.serial.txt` for the `[sched] *** IDLE-FALLBACK WEDGE`
block and pin the root cause.
