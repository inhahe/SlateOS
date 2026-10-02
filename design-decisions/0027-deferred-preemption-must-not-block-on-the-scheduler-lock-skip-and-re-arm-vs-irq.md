## 27. Deferred preemption must not block on the scheduler lock — skip-and-re-arm vs IRQ-off SCHED

**Date:** 2026-06-15

**Decided by:** Claude (autonomous). Correctness fix on the scheduler hot path,
discovered while driving the deferred benchmark suite to `BENCH_OK` after the Q7
landing (§26). Not a user-visible policy; mine to revisit if a better approach
appears.

**Context:**
With Q7's deferred preemption (§26), the only place an *involuntary* context
switch is initiated is `sched::do_deferred_preempt` → `preempt()` →
`schedule_inner()`, which takes `SCHED.lock()` (a plain `spin::Mutex`, **no**
interrupt masking). If a timer tick lands while the running task is *itself*
holding `SCHED`, the deferred preempt re-enters `SCHED.lock()` on the same CPU
and spins forever — the interrupted frame can never release the lock. The `cli`
added in §26 makes the hang unrecoverable. `bench_dashboard_api_status` is the
reliable reproducer: `api_status()` → `task_list()` holds `SCHED` across a heap
`Vec` collect over all tasks, run 1000× in a tight loop, so a tick almost
certainly lands inside a hold. The same hazard is a *latent* (tiny-window) risk
for every voluntary `SCHED` holder (`yield_now`, `block_current`), which also run
`schedule_inner` with interrupts enabled.

**Options considered:**
- **(A) Make `SCHED` an IRQ-safe lock** (acquire with interrupts disabled, like
  Linux's `rq->lock`). Most thorough: a timer can't fire mid-hold at all. But
  it is a sweeping change to ~40 lock sites and the context-switch path, forces
  every `SCHED` critical section (incl. `task_list`'s heap collect) to run with
  interrupts off (interrupt-latency cost), and *still* leaves the tiny
  SCHED-released-but-mid-`switch_context` window unprotected unless the lock is
  also held *through* the switch (a much larger restructuring). High risk on the
  safety-critical path for a benchmark-exposed bug.
- **(B) Per-CPU `preempt_count`** (Linux model): bracket every `SCHED` section
  with `preempt_disable`/`enable`, preempt only at count 0. Correct and general
  but the most invasive (touches all 40 sites; easy to miss one).
- **(C, chosen) Skip-and-re-arm in `do_deferred_preempt`.** Before preempting,
  check `SCHED.is_locked()`; if held, re-arm `NEED_RESCHED` and return, deferring
  the switch to the next tick. SCHED holds are short, so the preemption simply
  lands on a later tick where the task isn't holding the lock.

**Why C:**
- It fixes the **entire** "involuntary preempt while the interrupted context
  holds SCHED" deadlock class at the **single** point where involuntary
  preemption is initiated — including the latent voluntary-yield window — without
  touching any of the 40 `SCHED.lock()` call sites or the hot switch path.
- It is **consistent with an established pattern in this codebase**:
  `unthrottle_expired()` already uses `SCHED.try_lock()` and bails "because this
  runs in the timer ISR context." `do_deferred_preempt` services a flag the timer
  ISR set, so the same try/skip discipline is the natural fit.
- Preemption is inherently **best-effort/deferrable**: missing one tick's
  preemption because the scheduler lock is momentarily busy costs at most ~10 ms
  of extra runtime for the current task and is retried immediately on the next
  tick. There is no fairness or correctness loss.
- Imprecision is benign: `spin::Mutex::is_locked()` can't tell "held by this
  CPU's interrupted task" from "transiently held by another CPU." We
  conservatively skip in both cases. A cross-CPU false skip is just one deferred
  preemption — never a deadlock, since the other CPU *will* release the lock.

**Risks / tradeoffs:**
- Under sustained pathological `SCHED` contention a CPU could defer preemption
  for several ticks. In practice `SCHED` sections are short by design (§26's
  "single lock acquisition for the switch"); the bench's 1000-iter `task_list`
  loop still made full forward progress and the task was preempted normally
  between holds.
- This does **not** convert `SCHED` to IRQ-off, so interrupt-latency behavior is
  unchanged (a plus here). If a future need arises to hold `SCHED` across longer
  work, revisit option A/B.

**Validation:** with the guard in place the full `--bench` suite runs to
completion — `dashboard_api_status/health/metrics`, `isr_latency`, the scorecard,
and the `BENCH_OK` marker all appear ("Boot test PASSED"). Before the guard, the
suite hard-hung the moment it entered `bench_dashboard_api_status`.

**Where it lives:** `kernel/src/sched/mod.rs` `do_deferred_preempt` (the
`SCHED.is_locked()` skip-and-re-arm guard, ahead of the `cli`/`preempt`/`sti`
sequence).

**How to reverse:** delete the `if SCHED.is_locked() { … return; }` guard. This
reintroduces the deadlock for any involuntary preempt that lands while the task
holds `SCHED` (e.g. the dashboard benches), so reversal should only accompany a
move to option A or B.
