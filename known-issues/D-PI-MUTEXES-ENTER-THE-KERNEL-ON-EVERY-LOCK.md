## D-PI-MUTEXES-ENTER-THE-KERNEL-ON-EVERY-LOCK — a priority-inheritance mutex costs a syscall on every lock and unlock, where glibc's costs none uncontended (lane D, 2026-10-06)

**Status:** OPEN -- tech debt, waiting on lane A
(`requests/d-a-pi-futex-owners-taken-in-userspace-are-invisible-to-the-kernel.md`).

**In short:** a `PTHREAD_PRIO_INHERIT` mutex (one that lends a waiting
thread's priority to the thread holding it) is a futex word the kernel and
the library share. glibc takes a free one with one compare-and-swap in
userspace and enters the kernel only when the mutex is held. This library
enters the kernel every time instead, because the kernel records who holds
a word only when it saw the word taken -- and keeps a holder it has no
record of at priority lent to it after the lender has stopped waiting, among
other things the request lists. So every lock and every unlock of a PI
mutex here is a syscall. Correct, and slower than it needs to be.

**Where:** `posix/src/pthread.rs`, "Priority-inheritance mutexes" --
`pi_lock`, `pi_trylock`, `pi_lock_until` and `pi_unlock` call
`lowlevellock::futex_lock_pi` / `futex_lock_pi_until` / `futex_unlock_pi`
without trying the word themselves first. Mutexes of the other protocol are
untouched: they are the plain futex lock, a syscall only when contended.

**Who sees it:** programs that ask for PI mutexes -- audio and other
real-time code mostly. None in the tree yet; they were refused with
`ENOTSUP` until 2026-10-06, so nothing has got slower.

**The proper fix, once lane A's lands:** glibc's fast paths
(`__pthread_mutex_lock_full` and `__pthread_mutex_unlock_full`, PI cases).
- Lock: `compare_exchange(0, owner)` on `locked`, and the kernel only when
  that fails.
- Try: the same compare-and-swap first. The kernel is needed only for a word
  a dead owner left (`FUTEX_OWNER_DIED` with no owner), which the CAS of 0
  cannot take.
- Timed lock: the CAS before the deadline is read.
- Unlock: `compare_exchange(owner, 0)`, and `SYS_FUTEX_UNLOCK_PI` only when
  that fails (`FUTEX_WAITERS` or `FUTEX_OWNER_DIED` set).

`services/ctest-pi-mutex` check 6x is the test that says when the switch is
safe. It fails on a fast-path library until the kernel records owners.
