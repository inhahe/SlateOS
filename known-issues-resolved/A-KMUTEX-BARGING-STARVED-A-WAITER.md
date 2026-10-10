### A-KMUTEX-BARGING-STARVED-A-WAITER -- 2026-09-27 -- FIXED (lane A)

**In short:** the kernel's sleeping lock let a task that released it take it
straight back, before the task it had just woken could run. A task that
released and re-took it in a loop kept everyone else out for ever. That hung
boot rq15 for its last 20 minutes: one network connection sending without
pause held the lock every connection shares, and a quiet connection waiting
for it never got a turn.

**Where:** `kernel/src/sched/kmutex.rs`. `unlock` cleared the flag and woke one
waiter; the releaser's next `lock` won the fast-path CAS before the woken
waiter was scheduled, and the waiter went back to sleep. In design A of A-Q15
the netstack client's `SHARED_RING` is a `KMutex` held across a daemon
round-trip, and `ring_bench`'s busy sender, whose data is always ready, never
paused between round-trips: `idle_beside_busy`'s probe waited from boot second
~1260 to the 2400 s timeout, while the scheduler showed only the daemon and the
busy sender running.

**Fix:** lock handoff, as Linux's mutex has had since 2016
(`MUTEX_FLAG_HANDOFF`). A waiter that is woken and still loses sets `starving`;
the next unlock then hands the mutex over (`HANDOFF`) instead of freeing it,
and only a task that has already slept on the queue may take it
(`WaitQueue::wait_until_woken` tells it which it is). The fast path is
unchanged while nobody has been passed over. Self-test
`kmutex::self_test_no_starvation`: a holder that takes, sleeps holding, and
re-takes without pausing, 40 rounds; a waiter must have the mutex within three
rounds of asking. Without the handoff it waits all 40.
