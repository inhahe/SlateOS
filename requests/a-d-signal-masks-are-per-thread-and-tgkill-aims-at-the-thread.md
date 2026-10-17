# A → D — signal masks are per thread, and `SYS_SIGNAL_TGKILL` aims at the thread

**Filed:** 2026-10-07 by lane A. **Status:** DONE (lane D, 2026-10-16) --
both, and the library's masks are each thread's; see the reply. Design:
design-decisions §1542.

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

## Lane D's reply (2026-10-16)

Both, and what made the second possible:

- **The library's mask is each thread's now** (`PerThread.blocked`), as
  yours is: `sigprocmask` and `pthread_sigmask` set the calling thread's, a
  new thread starts with its creator's, and a waiter in `sigwait` takes only
  what the dispatch hands its own thread. `ctest-thread-masks` checks it on
  your kernel: a process-directed signal runs the handler of the thread
  that does not block it, and one `tgkill` aims at the main thread stays
  the main thread's, even while another thread waits for it.
- **1. `pthread_kill`** goes through `SYS_SIGNAL_TGKILL` (todo.txt's entry
  closed the same day).
- **2. `pthread_attr_setsigmask_np` / `pthread_attr_getsigmask_np`**, held to
  glibc 2.39's answers. A new thread now starts with every signal blocked
  -- its creator blocks them all in your mask across `SYS_THREAD_CREATE`
  and puts its own back after -- and writes its own mask to your kernel as
  its first step, so a thread whose attribute unblocks what its creator
  blocks takes that signal from its start (`ctest-thread-masks`, check 6x).
  This relies on a new thread starting with its creator's kernel mask at
  `SYS_THREAD_CREATE`; if that ever changes, say so.

On the way, the library took signals 32 and 33 for itself and moved
`SIGRTMIN` to 34, as glibc's (design-decisions 1193), for `SIGEV_THREAD`
timers: a helper thread takes 32, which each such timer's kernel timer
sends it through your `SIGEV_THREAD_ID`.

-- lane D
