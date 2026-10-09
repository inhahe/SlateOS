### [A] A-FUTEX-UNLOCK-PI-HANDOFF-RACED-A-TIMEOUT: a timed priority-inheritance lock could end up owned by the thread that had given up on it -- 2026-09-27
**Status:** FIXED on lane-a 2026-09-27, awaiting a boot. Found reading the
PI paths while keying them for process-shared futexes.

**In short:** a thread waiting for a priority-inheritance mutex with a
time limit could give up at the same moment the owner handed the mutex to
it. The kernel then recorded it as the owner anyway. It had already returned
"timed out" and would never unlock, so every other thread wanting that mutex
waited forever.

**Where.** `kernel/src/ipc/futex.rs` `futex_unlock_pi`:
1. It removed the chosen waiter from the queue under `PI_FUTEX_TABLE`.
2. It dropped the lock.
3. It registered the waiter as owner, taking the lock again.

A waiter whose timer fired between 2 and 3 took the lock, found itself
neither owner nor queued, and returned `TimedOut`. Step 3 then made it owner.
`lock_pi_inner`'s own comment claimed the lock closed exactly this race.
`exit_pi_owned_futexes` already registered in the same critical section.

**The fix.** `futex_unlock_pi` registers the new owner in the critical
section that dequeues it (`push_pi_owner`), as the exit path does.

**Test.** None deterministic: the window is between two lock holds on
another CPU. The fix makes the claimed invariant hold by construction, and
is recorded here because a boot cannot show it.
