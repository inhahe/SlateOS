### [D] B-D-PTHREAD-SYNC-POLLED-IN-1MS-STEPS — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/pthread.rs` — mutexes, condition variables, rwlocks,
barriers, `pthread_once`; now over `posix/src/lowlevellock.rs`.

**What it was.** None of pthread's blocking calls used a futex. A contended
`pthread_mutex_lock` spun 100 times and then slept in 1 ms steps, polling;
so did `pthread_cond_wait`/`timedwait`, `pthread_rwlock_rdlock`/`wrlock` and
`pthread_barrier_wait`; `pthread_mutex_timedlock` busy-yielded; a thread
waiting in `pthread_once` spun without yielding for however long `init` took.
Every hand-off cost up to a millisecond, and a waiter nobody would wake kept
waking up. Every lock, uncontended included, made a `SYS_TASK_ID` syscall
first — the whole cost of what should be one compare-and-swap
(`performance-targets.md`, "Futex wait/wake (uncontended)").

And three correctness faults beside it: a condition variable ignored its
attribute's clock, so a `CLOCK_MONOTONIC` deadline (seconds past boot) read
as real time had always passed and every timed wait returned `ETIMEDOUT` at
once; a barrier reused at once could release a thread with the round it did
not belong to (between the last arrival's reset of the count and its advance
of the generation); and `pthread_rwlock_timedrdlock`/`timedwrlock` did not
exist.

**Fix (design-decisions §1110).** A low-level futex lock (Drepper's
three-state mutex, glibc's `lll_lock`) under every mutex, the barrier and the
timed paths; a sequence-counter futex under condition variables, with the
waiter count kept so a signal nobody waits for costs no syscall; a futex
rwlock that answers `EDEADLK` to its writer; the calling thread's task id
cached in its per-thread block (reset in a `fork` child). The clock a
condition variable was made with is kept and used, and glibc 2.30's
`pthread_cond_clockwait`, `pthread_mutex_clocklock`,
`pthread_rwlock_clock{rd,wr}lock` and `sem_clockwait` exist, with the
`pthread_rwlock_timed*` pair. Host tests drive each with real threads.
