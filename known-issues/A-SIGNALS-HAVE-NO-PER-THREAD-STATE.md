### A-SIGNALS-HAVE-NO-PER-THREAD-STATE -- 2026-10-07 -- OPEN (lane A)

**Status:** OPEN (lane A). Long known in parts (todo.txt's `[B] 2026-08-16 --
pthread_kill delivers process-directed`, the Linux shim's `tgkill` notes);
written up whole when POSIX timers' `SIGEV_THREAD_ID` met it (§1540).

**In short:** on Linux each thread of a program has its own list of blocked
signals and its own pending signals, and a signal can be sent to one thread.
Here a process has one of each, shared by all its threads. Single-threaded
programs cannot tell. A multi-threaded one can: a thread that blocks a signal
blocks it for every thread, and a signal meant for one thread can be taken by
another.

**Where it shows:**

- `pthread_sigmask` / `rt_sigprocmask` set the process's mask. A program that
  blocks `SIGINT` in its worker threads and handles it in the main one blocks
  it everywhere. glibc's `pthread_create` blocks every signal, starts the
  thread, and restores -- while the new thread sets its own mask -- so the
  process's mask ends as whichever ran last. Worst case: glibc's `SIGEV_THREAD`
  timer helper, which blocks everything, can leave the whole program with all
  signals blocked.
- `tgkill`, `tkill`, `pthread_kill`, `rt_tgsigqueueinfo` and a POSIX timer's
  `SIGEV_THREAD_ID` reach the process, not the thread (`syscall::linux`
  `tgkill_common_value`, `proc::posix_timer::Notify::Signal::thread`). For
  glibc's `SIGEV_THREAD` timers that means the timer's signal (32, glibc's
  `SIGTIMER`) can be taken by a thread that has not blocked it -- whose
  default action for 32 ends the program -- instead of the helper thread
  waiting for it in `sigtimedwait`.
- `sigaltstack` is per process (native) or missing (Linux ABI, `sys_sigaltstack`
  accepts and ignores it -- todo.txt).
- `/proc/<pid>/status`'s `SigPnd` (the thread's own pending set) is always 0.

**Proper fix:** a per-thread signal state beside the process's, as Linux has:
the blocked mask, a private pending queue (records and timer entries), the
saved `sigsuspend` mask and the alternate stack become per thread; a
process-directed signal stays in the process's queue and is taken by any
thread that does not block it (and wakes one that can take it); a
thread-directed one -- `tgkill`, `SIGEV_THREAD_ID`, a synchronous fault --
goes to that thread's queue. The delivery checkpoint, `sigtimedwait`,
`signalfd` and `pause`/`sigsuspend` consult the current thread's state and the
process's. A new thread starts with its creator's mask; `fork`'s child with
the forking thread's. The native ABI needs a thread-scoped mask call and a
thread-directed send (todo.txt's `[B]` entry has the shape); lane D's
`pthread_sigmask` and `pthread_kill` then use them.
