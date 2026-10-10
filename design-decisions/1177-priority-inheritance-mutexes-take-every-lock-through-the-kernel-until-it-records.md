## 1177. Priority-inheritance mutexes are the kernel's PI futexes, and take every lock through the kernel until it records the owners it finds

**Date:** 2026-10-06
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** a program can ask for a mutex that, while a high-priority
thread waits for it, lends that priority to whichever thread holds it. Then
a low-priority holder is not kept off the CPU by medium-priority work while
the important thread waits behind it -- the "priority inversion" design.txt
asks the system to prevent. `pthread_mutex_init` refused such mutexes
(`PTHREAD_PRIO_INHERIT`) with `ENOTSUP` until today, though the kernel has
had the lending (PI futexes) all along. They are now the kernel's PI
futexes. Each lock and unlock goes through the kernel, which costs a syscall
apiece where glibc usually needs none. The cheap way would leave a
lent priority with its holder after the lender had stopped waiting.

**The choice: every lock through the kernel, for now.** The kernel documents
a fast path: take a free word with one compare-and-swap, and enter the
kernel only to wait. But it records an owner only when it saw the word
taken. A holder it has no record of keeps priority lent by a waiter that
gave up on a deadline, stops a chain of lenders, and loses priority lent
for one mutex when it unlocks another
(`requests/d-a-pi-futex-owners-taken-in-userspace-are-invisible-to-the-kernel.md`).

| | What changes | For | Against |
|---|---|---|---|
| Every lock and unlock in the kernel (chosen) | each PI lock and unlock is a syscall | the kernel has a record for every holder, so every loan is taken back when it should be; it is right today | slower than glibc's uncontended path -- a cost nobody pays yet, no program having had PI mutexes |
| glibc's fast path | an uncontended PI lock is one atomic instruction | glibc's speed; what Linux programs on the Linux ABI do anyway | a lent priority can stay with its holder for good -- silently, the failure the feature exists to stop |
| Keep refusing (`ENOTSUP`) | programs fall back to plain mutexes, or fail to start | nothing new to get wrong | no priority inheritance at all, though the kernel has it |

Switching to the fast path is a few lines once the kernel records the owners
it finds, as Linux's `attach_to_pi_owner` does. That is tracked in
`known-issues/D-PI-MUTEXES-ENTER-THE-KERNEL-ON-EVERY-LOCK.md`, and
`services/ctest-pi-mutex` check 6x is the test that says when it is safe.

**Two smaller calls, both easy to reverse:**
- `pthread_mutex_trylock` on an error-checking PI mutex that its caller
  holds answers `EBUSY` -- what POSIX and musl say, and what this library
  answers for every other mutex. glibc's PI path alone answers `EDEADLK`.
- A mutex that is not robust, whose owner died holding it, goes to the next
  locker, which goes on. The kernel hands it to the highest-priority waiter,
  or leaves it free with a dead-owner flag. Linux does the same for a
  waiter; a later locker it answers `ESRCH`, on which glibc sleeps forever.
  POSIX leaves it open ("can lead to deadlocks"). Sleeping forever on
  purpose seemed the worse answer.

**Not done, and why:** `PTHREAD_PRIO_PROTECT` (priority ceilings) needs a
mapping from POSIX's `SCHED_FIFO` priorities to the kernel's 0-31 levels,
which this library does not have -- `pthread_setschedparam` grants no
real-time priority at all. Robust mutexes need the kernel to keep a robust list
for its own ABI; today only the Linux ABI can register one. Both are still
refused with `ENOTSUP`.
