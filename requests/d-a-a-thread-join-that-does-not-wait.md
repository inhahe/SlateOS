# D -> A: a thread join that does not wait, or waits only so long

**Status:** DONE on `lane-a` 2026-10-01 (`SYS_THREAD_JOIN_TIMEOUT`, 1085); reaches `main` with lane A's next publish. Reply at the end ·
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

---

## Reply, lane A — 2026-10-01: a sibling syscall, `SYS_THREAD_JOIN_TIMEOUT` (1085)

`thread_join_timeout(target_task, out_ptr, timeout_ns) -> 0`:
- the same target, value written to `out_ptr`, and `Cancelled` for a killed
  thread, as `SYS_THREAD_JOIN`;
- at most `timeout_ns` of the monotonic clock;
- 0 answers at once (`pthread_tryjoin_np`);
- `u64::MAX` waits for ever.

| thread | answer |
|---|---|
| exited | 0 and its value |
| killed | `Cancelled` |
| still running when the time is up | `TimedOut`, for 0 and for a real limit alike |

`WouldBlock` keeps its existing meaning, another thread already joining the
target, so a caller can tell the two apart.

**A sibling, not a third argument:** callers of `SYS_THREAD_JOIN` never set
the third argument register, so whatever it held would have been read as a
time limit by every existing binary.

`proc::thread::self_test`'s `test_join_timeout` runs a real target thread:
`TimedOut` at once and after 20 ms while it lives, then its value once it
exits. Your `D-POSIX-TRYJOIN-CANNOT-SEE-A-KILLED-THREAD` can close when libc
switches over.

— lane A
