### TD-POSIX-TEST-CAP-STATE-SHARED-ACROSS-TEST-THREADS. Cap-gated posix tests fail at random under the parallel runner because the capability words are process-global — 2026-08-12 — ✅ FIXED 2026-08-12

**What.** `cargo test -p posix --target x86_64-pc-windows-gnu` intermittently
failed a *single*, *different* cap-related test per run while the same test
passed 3/3 in isolation. Two victims observed before the fix:
`epoll::tests::test_phase199_timerfd_cap_restore_re_enables_alarm`
("must succeed after cap restored", `posix/src/epoll.rs:6759`) and
`socket::tests::test_phase201_bind_privileged_port_with_cap_ok`. Interleaved
clean runs of 20,128 passed made it look like noise; it was not.

**Root cause.** `sys_capability`'s three capability sets were six process-global
`AtomicU32`s. A test that drops a cap to exercise an unprivileged path mutates
state that *every concurrently running test* shares. The crate already knew
this: a global `CAP_TEST_LOCK` mutex existed, taken by all 46 test-only
`CapGuard`s, and its comment recorded "~150 spurious test failures per run"
without it. But a mutex only protects code that takes it, and the surviving
failures were precisely the tests that **read** the cap state without holding
the guard — asserting `has_capability(...)` or that a privileged operation
succeeds, while another thread had that cap dropped. `test_phase199_…_restore_…`
was the sharpest case: it holds the guard in an inner scope, then asserts the
cap is back *after* the scope ends, i.e. after releasing the lock.

**Why the fix is structural, not another guard.** Adding `CapGuard::snapshot()`
to the dozen-odd reader tests would have worked and been wrong: it leaves the
hazard in place and makes correctness depend on every future test author
remembering an invisible rule. The capability words are only process-global
*on the target*, where that is what they model — a real process's threads do
share its privileges. On the host the crate exists solely to be unit-tested, so
there is no reason for one test thread's voluntary privilege drop to be visible
to another.

**Fix.** `sys_capability` grew a `store` module with two cfg'd bodies behind a
`CapWords` load/store pair: `AtomicU32`s as before on `target_os = "none"`, and
a `thread_local!` `Cell<CapWords>` on the host. This is the same remedy, and the
same reasoning, as TD-POSIX-TEST-PARALLEL (`perthread.rs`) — cited in the new
module comment. Each test thread now starts from the cold-boot default and
cannot observe or disturb another's caps, so the entire failure class is
impossible rather than merely defended against, and the unguarded reader tests
became correct without being touched.

`CAP_TEST_LOCK`/`CapTestLockGuard` and all 138 use sites were then deleted: with
per-thread state the lock guards nothing, and keeping it would have serialised a
large slice of a 20k-test suite behind a mutex whose documented rationale was no
longer true. Removing it doubles as the proof — if the words were still shared,
the ~150 failures its own comment describes would have returned immediately.

**Verified.** 45 consecutive suite runs after the change with the lock fully
removed: 44 clean at 20,128 passed, 1 failure whose identity was not captured
and which did not recur in 37 further targeted runs. Both original victims are
green. `cargo build` clean for the target and host.

**Residual — identified, see the entry below.** That one uncaptured failure was
real: a 60-run hunt caught three more failures belonging to two *different*
process-global statics, not to the caps. They are covered by
TD-POSIX-TEST-PGRP-AND-TIMER-STATE-SHARED-ACROSS-TEST-THREADS.
