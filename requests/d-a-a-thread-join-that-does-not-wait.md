# D -> A: a thread join that does not wait, or waits only so long

**Status:** OPEN ·
**Date:** 2026-09-28 by lane D ·
**Affects:** `kernel/src/syscall/` (yours); `posix/src/pthread.rs` (mine)

## In short

glibc has two ways to join a thread without committing to wait for it:
`pthread_tryjoin_np` ("has it finished? don't wait") and
`pthread_timedjoin_np` ("wait, but only until this time"). The libc has them
now, but it has to guess whether a thread has finished from a flag the
thread sets on its way out, because `SYS_THREAD_JOIN` only answers once the
thread is gone and waits until then. A thread killed by a fault never sets
the flag, so to these two it looks alive for ever -- while `pthread_join`,
which asks you, returns at once with `PTHREAD_CANCELED`.

## What I am asking for

A deadline on `SYS_THREAD_JOIN`: a third argument, nanoseconds of
`CLOCK_MONOTONIC` to wait at most, with the current behaviour for
`u64::MAX` (the value every existing caller would pass once the argument
exists -- or keep the two-argument form as it is and add a sibling, as you
prefer). 0 means "answer now". The answers the libc needs:

| thread | answer |
|---|---|
| exited | as now: 0 and its value |
| killed | as now: `Cancelled` |
| still running when the time is up | a distinct error -- `WouldBlock` for 0, `TimedOut` otherwise, or one code for both |

The libc converts `pthread_timedjoin_np`'s `CLOCK_REALTIME` deadline to a
relative wait itself.

## What happens until then

The two functions poll the thread's slot every millisecond, which is right
for every thread that ends by `pthread_exit` or by returning, and wrong only
for one killed outright -- recorded as
`known-issues.md` -> `D-POSIX-TRYJOIN-CANNOT-SEE-A-KILLED-THREAD`.
Nothing gets worse if this is never answered.
