### B-FUTEX-TOWAKER-LOSTWAKE. Futex timeout self-test waker could lose its wakeup (wake before waiter parked) → spurious `TimedOut` under shifted boot timing — FIXED 2026-07-14

**Where:** `kernel/src/ipc/futex.rs`, `timeout_waker_task` /
`test_timeout_woken_before_deadline`.

**Bug:** the waker calls `futex_wake` **without changing the futex word**, so
correctness depended on the waiter being parked before the wake. If the wake
fired first, the waiter re-checked its (unchanged) expected value, parked anyway,
and the earlier wake was lost — the wait then ran the full 500 ms and returned
`TimedOut`, failing the test. A fixed number of `yield_now()`s cannot guarantee
the ordering; the `net.userspace` switch-on boot shifted task-id/scheduler timing
enough to expose it (manifested as whichever timeout self-test hit the bad
interleave — channel/eventfd failures in the same window were instead
daemon-starvation, fixed separately by deferring the persistent daemon past
POST). **Fix:** the waker now retries `futex_wake` until it reports it actually
woke a waiter (bounded spin), which is deterministic regardless of interleave.
