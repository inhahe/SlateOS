### [A] A-FUTEX-WAKE-STOPPED-AT-32: waking "everyone" on a futex woke at most 32 waiters -- 2026-09-27
**Status:** FIXED on lane-a 2026-09-27, awaiting a boot. Found reading the
futex paths while keying them for process-shared futexes.

**In short:** when a program wakes all the threads waiting on something -- a
condition variable's broadcast, a barrier letting its threads through -- the
kernel woke at most 32 of them. The rest stayed asleep, and the kernel told
the program 32 as if that were all of them. A program with more than 32
threads waiting on one condition hung.

**Where.** `kernel/src/ipc/futex.rs` `futex_wake_bitset` gathered the tasks to
wake in a 32-slot array and stopped when it was full. Every wake goes
through it once, and nothing above it loops:
- native `SYS_FUTEX_WAKE`;
- Linux `FUTEX_WAKE` and `FUTEX_WAKE_BITSET`;
- futex2's `futex_wake`;
- `FUTEX_WAKE_OP`.
`requeue_inner`'s wake phase had the same array.

**The fix.** Both collect into a `Vec`, reserved one entry at a time. An
allocation failure ends the wake early and the returned count says how many
were woken.

**Test.** `futex::test_wake_many`: forty kernel tasks park on one word, and
a wake of `u32::MAX` must report forty and let all forty finish.
