### B-THREAD-JOIN-EXIT-RACE. `thread::join` could park forever on a thread that exited between the liveness check and the waiter registration — FIXED 2026-08-13

**How it was found.** Not from a failing boot: it came out of the audit that
followed B-PTHREAD-JOIN-LOST-CTID above, checking every piece of state
`on_thread_exit` consults for the same "the other side registers too late"
shape. `THREAD_JOIN_WAITERS` is the one such table registered by the *joiner*
rather than by the spawner, so it needed the mirror-image argument.

**The window.** `proc::thread::join` did:

1. take `THREAD_OWNERS`, confirm the target exists and shares our process,
   **drop the lock**;
2. take `THREAD_JOIN_WAITERS`, insert `target -> caller`, drop it;
3. park in a loop whose *only* exit condition is that entry being removed.

`on_thread_exit` drains `THREAD_JOIN_WAITERS` (waking the joiner) **before** it
removes the task from `THREAD_OWNERS`. So a target that exits between steps 1
and 2 drains an empty waiter map, and the entry inserted at step 2 is one that
nothing will ever remove — the loop at step 3 never terminates and the joiner
is stuck for the life of the process.

This is a genuine hang, but a much narrower one than the ctid bug: it needs a
preemption inside a two-instruction-wide gap between dropping one lock and
taking another, and it only affects the native `SYS_THREAD_JOIN` path (glibc
`pthread_join` goes through the ctid futex instead), which is why no soak has
caught it.

**Fix.** Register-then-recheck, the same idiom `futex_wait_bitset` already uses
for the signal-arrival race. After publishing the waiter entry, re-check
`THREAD_OWNERS`; if the target is gone, the exit already ran, so withdraw the
entry (only if it is still ours — a racing `on_thread_exit` may have removed it
and issued a real wake, which must not be swallowed) and return the recorded
outcome, or `NoSuchProcess` if none — matching what the pre-existing
target-not-registered branch above already returns. The ordering that makes
this correct is the one noted above: waiters are drained strictly before the
`THREAD_OWNERS` removal, so "gone from `THREAD_OWNERS`" implies "the waiter
drain has already happened."

Neither lock is ever held while taking the other, so no new lock-order edge is
introduced.

**Files.** `kernel/src/proc/thread.rs` (`join`).
