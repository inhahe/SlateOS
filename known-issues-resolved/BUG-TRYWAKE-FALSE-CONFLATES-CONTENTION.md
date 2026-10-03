### BUG-TRYWAKE-FALSE-CONFLATES-CONTENTION. `sched::try_wake` returned `false` both for "lock contended, retry" and for "already recorded as `pending_wake`", so every wake aimed at a not-yet-parked task queued a duplicate deferred wake that later fired against an unrelated park — FIXED 2026-07-27

**Where:** `kernel/src/sched/mod.rs` — `try_wake()` (~1874) and its ~20 call
sites, all of which spell the same idiom:

```rust
if !sched::try_wake(tid) {
    sched::defer_wake(tid);
}
```

(ipc: pipe, eventfd, stream_socket, channel, futex ×4, semaphore, service,
completion, timerfd; plus proc::signal ×2, fs::notify, ioapic, syscall::linux
×2, and `process_deferred_wakes` itself.)

**Bug.** `try_wake` returned `false` in three materially different
situations, and the idiom above cannot tell them apart:

1. `SCHED.try_lock()` failed — nothing was done, retry is *required*;
2. the lock was taken but the task was `Running`/`Ready`, so `pending_wake`
   was set — the wake is **fully accounted for**, retry is *wrong*;
3. the task did not exist — nothing to wake, retry can *never* succeed.

Case 2 is not a corner case: it is the ordinary register-then-recheck
interleaving, i.e. any wake that lands before the target has actually parked.
Every such wake therefore also enqueued a **duplicate** into the 32-slot
deferred-wake queue. That duplicate then fired later against whatever the
task was doing at the time — waking an unrelated, *subsequent* park early via
`drain_deferred_wakes_locked`, or (via `process_deferred_wakes`) re-setting
the sticky `pending_wake` bit so some other `block_current()` returned
without blocking. This is precisely the "stale `pending_wake` consumed by an
unrelated later park" lead recorded under
`BUG-DASH-CMDSUB-INTERMITTENT-HANG`.

Case 3 leaked resources outright: `process_deferred_wakes` only frees a slot
when `try_wake` returns `true`, so a deferred wake for a dead task could
never be cleared. One leaked slot pinned `DEFERRED_WAKES_PENDING` forever,
forcing a full 32-slot rescan on every timer tick, and enough of them would
exhaust the queue.

**Impact.** Spurious early returns from `block_current()`. Park loops that
re-check their condition degrade to a harmless spin, which is why this stayed
latent; any single-shot `block_current()` would proceed as if its event had
occurred.

**Found by:** following lead (b) of `BUG-DASH-CMDSUB-INTERMITTENT-HANG` after
the single-waiter-slot sweep (lead (a)) closed.

**Fix.** `try_wake`'s contract is now unambiguous: it returns `false`
**only** for case 1, and `true` for every case in which the wake has been
accounted for (2 and 3 included). No call site needed to change — the
existing `if !try_wake { defer_wake }` idiom becomes exactly correct, and
`process_deferred_wakes` now frees slots that could previously never be
freed. The body was also flattened to `let … else` so each case returns
explicitly instead of falling through to a shared trailing `false`, which is
what allowed the three to be conflated in the first place.

This is the same fix, and the same shape, as the one already applied to the
*sleeper* queue: `wake_expired_sleeper()` (~4949) returns
`SleeperWake::Retry` **only** on lock contention, and `SleeperWake::Release`
for both the not-blocked and the task-gone cases, after an identical slot
leak. The deferred-wake queue had simply not been given the same treatment.

**Regression test:** `test_try_wake_contract()` in `kernel/src/sched/mod.rs`
(wired into `sched::self_test()`). Asserts `try_wake` on a non-existent task
returns `true`; asserts `try_wake` on the *running* caller returns `true`,
then that the resulting `pending_wake` token makes the very next
`block_current()` return instead of parking, then that the token was consumed
(read directly out of the task table rather than by parking again, which
would genuinely block). Boot-verified: `[sched]   try_wake contract (retry
only on lock contention): OK`.
