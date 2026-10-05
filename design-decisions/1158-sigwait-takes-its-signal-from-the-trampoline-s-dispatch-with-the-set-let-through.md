## 1158. `sigwait` takes its signal from the trampoline's dispatch, with the set let through by the kernel's mask alone -- not from the kernel's pending set, and not by unblocking it in the program's mask

**Date:** 2026-09-30
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** `sigwait`, `sigtimedwait` and `sigwaitinfo` were stubs that
took no signal. They now take one the way the rest of this library handles
signals: the kernel delivers it to the trampoline, and the dispatch, instead
of running a handler, hands it to the thread waiting for it. While a thread
waits, only the kernel's copy of the signal mask lets its set through; the
program's own mask -- what `sigprocmask` reports, and what a handler sets
and restores -- still blocks it.

### Alternative 1: take the signal from the kernel's pending set

Ask lane A for a native call that takes a pending signal of a set without
delivering it, as `rt_sigtimedwait` already does for Linux programs here
(`kernel/src/syscall/linux.rs`, over `take_pending_in_mask`).

- **For:** the set stays blocked throughout and no mask changes at all; the
  kernel's record of the signal -- its sender, its code, its value -- could
  come with it, where the native frame brings the number alone; and the
  kernel arbitrates between several waiters.
- **Against:** a kernel change with the lane waiting on it, for a call the
  library can serve today from what it has. The call would also have to end
  for a handler that runs on the waiting thread, as glibc's does, which is
  more than taking a signal.

### Alternative 2: unblock the set in the program's mask while waiting

What this change's first draft did: note the mask, clear the set from it,
wait, put the noted mask back.

- **For:** no second idea of the mask to keep in step.
- **Against:** the mask is one word for the whole process, and every other
  writer of it notes and restores it too. A handler that ends on another
  thread while the wait is under way puts back the mask it found -- the set
  blocked -- and the waiter sleeps through its own signal. One that began
  during the wait and ends after it reopens the set, and the next signal of
  it runs its disposition -- the default one, fatal for most -- instead of
  waiting, pending, for the next `sigwait`. And `sigprocmask` would report
  the set unblocked, which on Linux a waiting thread's mask never is.

### What was done

The kernel's mask is the program's less the union of the sets the waiters
have published (`kernel_mask_now`), written again whenever either changes --
and once more if another change raced the write, which a generation count
detects, since two writers' masks can land in either order. The table of
waiters is lock-free, the dispatch running in signal context on any thread:
it hands a signal to a waiter free to take it before keeping it for one that
has taken another, and a waiter that has stopped takes nothing.

### Consequences

- A signal of a set waited for is taken, with no handler run, by the thread
  waiting for it, whichever thread the kernel delivers it to.
- The dispatch no longer leaves a blocked signal pending while the kernel's
  mask may still let it through (`keep_pending`): the kernel would have
  delivered it straight back into the same dispatch, and again from there.
- A child of `fork` forgets its parent's waiters, whose threads it lacks.
- `sigwaitinfo`'s siginfo carries the number only, as long as the native
  frame carries nothing more.
- At most 32 threads wait at once (`ACCEPTORS`); a 33rd is told `EAGAIN`.

**Where:** `posix/src/signal.rs` (`Acceptor`, `hand_to_a_waiter`, `accept`,
`kernel_mask_now`, `sync_kernel_blocked_mask`, `keep_pending`,
`forget_waiters_after_fork`); `posix/src/process.rs` (`fork_raw`).
