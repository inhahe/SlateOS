# A → D — signal masks are per thread, and `SYS_SIGNAL_TGKILL` aims at the thread

**Filed:** 2026-10-07 by lane A. **Status:** OPEN (for lane D: two library
changes become possible, none is required). Design: design-decisions §1542.

## In short

Each thread now has its own blocked-signal mask, its own pending signals and
its own alternate signal stack, and a signal can be sent to one thread. For
the C library that means:

| call | was | is now |
|---|---|---|
| `SYS_SIGNAL_MASK` | the process's one mask | the **calling thread's** mask; a new thread starts with its creator's |
| `SYS_SIGNAL_PENDING` | the process's pending set | the calling thread's view: sent to the process, or to it alone |
| `SYS_SIGNAL_ALTSTACK` | the process's stack | the **calling thread's** stack (a new thread starts with none; `fork` passes the forking thread's) |
| `SYS_SIGNAL_TGKILL` | posted to the process | posted to **that thread**: it waits on the thread's queue, is taken by it, and is blocked when it blocks it |
| `SYS_SIGNAL_SEND`, `SYS_SIGNAL_QUEUE` | process | unchanged: process-directed; taken by whichever thread does not block it |

## What the library can now do

1. **`pthread_kill` can be thread-directed.** todo.txt's `[B] 2026-08-16 --
   pthread_kill delivers process-directed` says it sends
   `SYS_SIGNAL_SEND(getpid(), sig)` for want of a thread-directed call;
   `SYS_SIGNAL_TGKILL(getpid(), tid, sig)` is that call now, and step 2 of
   that entry's fix (delete the paragraph documenting the degradation)
   follows.
2. **`pthread_sigmask` is per thread already** -- it was `sigprocmask`, and
   `sigprocmask` now sets the calling thread's mask, as on Linux -- so
   `pthread_attr_setsigmask_np` / `pthread_attr_getsigmask_np` (the two
   names `D-POSIX-LIBC-LACKS-WHAT-GLIBCS-HEADERS-DECLARE` lists as waiting on
   per-thread masks) can be built: the new thread sets its mask first thing.

## Worth checking

Anything in the library that relied on one thread's `sigprocmask` affecting
the others (blocking a signal process-wide from one thread) now affects only
that thread, as on Linux.

-- lane A
