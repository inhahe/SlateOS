### B-EVENTFD-TOTEST-SHORTTIMEOUT. Eventfd "signaled-before-expiry" self-test used a 500 ms reader timeout + fixed-yield polling → spurious `TimedOut` (`got 18446744073709551615`) under boot-time scheduler contention — FIXED 2026-07-15

**Where:** `kernel/src/ipc/eventfd.rs`, `eventfd_timeout_reader_task` /
`test_timeout_signaled`.

**Observed:** caught during the post-serial-fix wedge-soak
(`build/hang-catches/soak-20260715-022705-iter12`): boot reached BOOT_OK but the
eventfd timeout self-test failed —
`[eventfd]   FAIL: timeout_signaled: got 18446744073709551615`
(`18446744073709551615` = `u64::MAX`, the reader's error sentinel), then
`[FATAL] Eventfd timeout self-test failed: InternalError`. Intermittent: 1 of
~13 armed boots in that soak; all other eventfd sub-tests passed.

**Root cause:** this is a *test-timing* fragility, **not** a lost-wakeup — the
scheduler's `pending_wake` protection (`sched::wake` sets `pending_wake` on a
not-yet-blocked task; `block_current` consumes it) correctly closes the
register-then-park race in `read_timeout`. Instead, the reader parked with only a
**500 ms** timeout while the main test task signaled it after a fixed
`yield + sleep_ms(5)`. During the busy boot self-test phase, transient scheduler
contention can delay the *signaling* task past the reader's 500 ms deadline, so
the reader legitimately times out (`read_timeout` → `Err(TimedOut)` → stores
`u64::MAX`) even though the eventfd signal path is correct. The main task's
fixed post-write `yield×2 + sleep_ms(5)` result check compounded the fragility
(it assumed the reader is always scheduled to completion within that window).
Same class as B-FUTEX-TOWAKER-LOSTWAKE and the channel `recv_timeout` flake.

**Fix:** (a) give the reader a generous **5 s** timeout — many orders above the
~5 ms the driver takes to signal — so the timeout can never fire under normal or
momentarily-starved scheduling (only a genuinely broken signal path fails it);
(b) replace the fixed post-write yields/sleeps with a **bounded poll loop**
(200 × `yield + sleep_ms(5)`, ~1 s cap) that waits for the reader to store its
result, so a real signal-path bug still fails deterministically in ~1 s rather
than depending on exact interleave.
