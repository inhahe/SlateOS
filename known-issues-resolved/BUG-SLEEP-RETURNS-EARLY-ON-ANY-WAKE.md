### BUG-SLEEP-RETURNS-EARLY-ON-ANY-WAKE. `sleep_ns`/`sleep_until_tick` parked once and returned, so *any* wake — an unrelated `sched::wake`, or a stale `pending_wake` token — ended the sleep early: a `sleep(5s)` could return after a millisecond — FIXED 2026-07-27

**Where:** `kernel/src/sched/mod.rs` — `sleep_until_tick()` (~4566) and
`sleep_ns()` (~4876), plus their `sleep_ms`/`sleep_us` wrappers.

**Bug.** Both arm a timer (a sleep-queue slot / an hrtimer), call
`block_current()` **once**, and return. `block_current()` returning is not
evidence that the deadline arrived: any `wake`/`try_wake` aimed at the task
releases it, including a stale `pending_wake` token (see
`BUG-TRYWAKE-FALSE-CONFLATES-CONTENTION`) and the `pending_wake` that
`wake_expired_sleeper` deliberately sets when it finds an early-woken
sleeper. So the sleep duration was a *maximum*, not a duration — while every
caller reads it as a duration:

* `sys_sleep` (`handlers.rs`) and `nanosleep`'s no-signal-context fallback
  (`linux.rs`, whose own doc comment calls `sleep_ns` "non-interruptible");
* io_ring `IO_OP_TIMEOUT`, which promises "sleep for the specified duration";
* `sched::supervisor` and `ktimer` self-tests, which sleep to let real ticks
  elapse and would otherwise assert against a clock that had not moved.

**Fix — split the contract instead of picking one.** This mirrors Linux,
where `schedule_timeout()` may return early and `msleep()` loops around it:

* `sleep_ns` / `sleep_until_tick` / `sleep_ms` now **loop on the clock**,
  re-arming for the *remaining* time until the deadline genuinely arrives.
  The `sleep_ns` deadline is captured *before* arming the hrtimer, so it can
  never be later than the timer's own expiry — a real timer wake can't be
  mistaken for a spurious one and re-parked against a timer that already
  fired.
* `sleep_ns_interruptible` / `sleep_until_tick_interruptible` /
  `sleep_ms_interruptible` keep the old "deadline or earlier wake" behaviour.

**Callers that genuinely want the early wake** (audited one by one, all
switched to the `_interruptible` variants — this is why the old behaviour
could not simply be replaced):

1. `WaitQueue::wait_until_timeout` / `wait_until_timeout_ns`
   (`sched/waitqueue.rs`) — a `wake_one()` before the deadline must return
   control so the condition is re-checked. Looping would degrade every
   timed condition wait into a full-timeout wait.
2. `kswapd` (`mm/kswapd.rs`) — its own comment says the ~1 s sleep "is
   interruptible via `wake_kswapd()` → `try_wake()`". Looping would delay
   reclaim by up to a full interval under memory pressure.
3. `interruptible_wait_slice` (`syscall/linux.rs`, poll/select/epoll) —
   registers as a signal waiter before parking specifically so
   `set_pending` → `try_wake` cuts the slice short.

**Found by:** extending the `BUG-SINGLE-SHOT-PARK-FABRICATES-EVENT` audit
into `sched/` itself, which was the last unaudited group of
`block_current()` sites.
