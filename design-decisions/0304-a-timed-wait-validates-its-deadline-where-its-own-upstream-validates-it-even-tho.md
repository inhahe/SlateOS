## §304 — A timed wait validates its deadline where *its own* upstream validates it, even though they all share one predicate

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)
**Lane:** B

### The problem

None of `pthread_cond_timedwait`, `pthread_mutex_timedlock` or
`sem_timedwait` checked `abstime->tv_nsec` at all. Adding the check is not the
decision; *where* to add it is. glibc calls one shared inline predicate,
`valid_nanoseconds` (`include/time.h:517`), from all three — which makes it
look as though the natural port is one helper called from one place in each
function, namely the top. That port would be wrong in two of the three.

### What the source says

| Function | glibc source | position of the check |
|---|---|---|
| `pthread_cond_timedwait` | `nptl/pthread_cond_wait.c:635` | **first statement**, above the mutex release |
| `sem_timedwait` | `nptl/sem_timedwait.c:28` | **above** `__new_sem_wait_fast` |
| `pthread_mutex_timedlock` | `nptl/pthread_mutex_timedlock.c:221` | **inside the contended branch**, under the comment "We are about to block; check whether the timeout is invalid" |
| `pthread_rwlock_{rd,wr}lock_full64` | `nptl/pthread_rwlock_common.c:292` | **first statement**, with a comment recording that this was *changed* from lazy to eager |

The observable consequence: `pthread_mutex_timedlock` on an **uncontended**
mutex with `tv_nsec = 1e9` returns 0, while `sem_timedwait` on a semaphore
with a **positive count** and the same timespec returns `EINVAL`. Same
predicate, same class of caller error, opposite answers.

POSIX permits both — "the validity of the abstime parameter need not be
checked if the lock can be immediately acquired" — which is precisely why the
implementations diverged and why the rwlock comment exists: glibc took the
allowance for mutexes, declined it for rwlocks and semaphores, and documented
the switch.

The fourth site is a different predicate entirely. `mq_timedsend` and
`mq_timedreceive` are bare syscalls, so the kernel validates: `prepare_timeout`
(`ipc/mqueue.c`) → `timespec64_valid` (`include/linux/time64.h`), which
rejects `tv_sec < 0` as well as an out-of-range `tv_nsec`. glibc's predicate
never inspects `tv_sec`, so for the pthread and semaphore waits a negative
`tv_sec` is a deadline in the past — `ETIMEDOUT`, not `EINVAL`.

### The decision

1. **Port the predicate once** (`time::valid_nanoseconds`, glibc's definition
   verbatim) but **place each call where its own upstream places it** —
   eagerly in `pthread_cond_timedwait` and `sem_timedwait`, lazily in
   `pthread_mutex_timedlock`.

2. **Do not share the predicate with `mqueue`.** `deadline_from_timespec`
   keeps its own `tv_sec < 0` test on top of `valid_nanoseconds`, because it
   is implementing `timespec64_valid`, not `valid_nanoseconds`.

The alternative to (1) was to check eagerly everywhere: one rule, simpler to
state, and arguably *better* — a caller who passes a malformed timespec has a
bug whether or not the lock happened to be free, and the lazy version makes
that bug appear only under contention, i.e. intermittently. Rejected anyway.
The whole point of this layer is that a program built against glibc behaves
the same here, and a real program does depend on the lazy behaviour by
accident: an uncontended `pthread_mutex_timedlock` with a sloppily-computed
deadline is a common pattern that works on Linux, and failing it would be a
regression visible only to us. The eager version is a defensible libc design
and not the one we are implementing. (§303 made the same call for a different
reason: the order of validations is part of the ABI.)

The alternative to (2) was one predicate for everything, taking the union or
the intersection. Rejected: the union breaks `sem_timedwait` with a
past deadline (`EINVAL` instead of `ETIMEDOUT`), the intersection breaks
`mq_timedsend` with `tv_sec = -1` (a very long wait instead of `EINVAL`).
Two rules that differ by one line and cannot be merged.

### Consequences

Anyone adding `pthread_cond_clockwait`, `sem_clockwait` or the
`pthread_rwlock_{timed,clock}{rd,wr}lock` family must look up that function's
own placement rather than copying a neighbour's. The rwlocks are **eager**,
and the clock-taking variants additionally reject an unsupported `clockid`
(`futex_abstimed_supported_clockid`) *before* the nanoseconds check.

The general form of the rule, which outlives this entry: a shared helper
upstream is evidence about the *predicate*, not about the *control flow*.
§303's "port the upstream helper rather than the check" is necessary and not
sufficient.
