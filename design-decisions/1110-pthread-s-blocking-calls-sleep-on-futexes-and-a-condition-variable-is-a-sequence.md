## 1110. pthread's blocking calls sleep on futexes, and a condition variable is a sequence counter

**Date:** 2026-09-26
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** threads waiting for a lock, a condition variable, a
read-write lock or a barrier used to wake up every millisecond to look again;
now they sleep in the kernel until the thread that releases them wakes them,
and an uncontended lock never enters the kernel at all (`known-issues.md` →
`B-D-PTHREAD-SYNC-POLLED-IN-1MS-STEPS`). The locks are glibc's algorithm. The
condition variable is not, and that is the choice recorded here.

glibc's condition variable (nptl/pthread_cond_wait.c) splits waiters into
two groups so that a signal can only wake a thread that was already waiting
when it was sent, and none is woken by a signal meant for another. The
simple alternative — the one Rust's standard library uses on Linux — is a
sequence number: a signal advances it and wakes a sleeper; a waiter sleeps
until the number it saw moves.

| Option | For | Against |
|---|---|---|
| (a) **sequence counter (chosen)** | a page of code, easy to see correct; one futex; no syscall when nobody waits | a thread that starts waiting just after a signal can take that signal's wake-up ("stolen" wake-ups), which POSIX allows but glibc avoids; a broadcast wakes every waiter to contend for the mutex |
| (b) glibc's two-group algorithm | no stolen wake-ups | several hundred lines of subtle code, whose correctness argument is its own paper |

Callers must loop on their predicate anyway — POSIX permits spurious
wake-ups — so (a)'s stolen wake-ups change timing, not correctness. The
rwlock prefers readers, as glibc's default (`PTHREAD_RWLOCK_PREFER_READER_NP`)
does. Revisit (b) if a workload shows a fairness problem.
