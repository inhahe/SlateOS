### A-SIGNALS-HAVE-NO-PER-THREAD-STATE -- 2026-10-07 -- FIXED the same day (lane A); the Linux alternate stack OPEN separately

**Status:** FIXED on lane-a-wip 2026-10-07 (design-decisions §1542), awaiting
a boot. Long known in parts (todo.txt's `[B] 2026-08-16 -- pthread_kill
delivers process-directed`, the Linux shim's `tgkill` notes); written up whole
when POSIX timers' `SIGEV_THREAD_ID` met it (§1540), and fixed the same day.
What is left of the alternate-stack half is the Linux ABI's, which has no
`sigaltstack` at all: `A-LINUX-PROGRAMS-HAVE-NO-ALTERNATE-SIGNAL-STACK`.

**In short:** on Linux each thread of a program has its own list of blocked
signals and its own pending signals, and a signal can be sent to one thread.
Here a process had one of each, shared by all its threads. Single-threaded
programs could not tell. A multi-threaded one could: a thread that blocked a
signal blocked it for every thread, and a signal meant for one thread could
be taken by another.

**Where it showed (before the fix):**

- `pthread_sigmask` / `rt_sigprocmask` set the process's mask. A program that
  blocked `SIGINT` in its worker threads and handled it in the main one
  blocked it everywhere. glibc's `pthread_create` blocks every signal, starts
  the thread, and restores -- while the new thread sets its own mask -- so the
  process's mask ended as whichever ran last. Worst case: glibc's
  `SIGEV_THREAD` timer helper, which blocks everything, could leave the whole
  program with all signals blocked.
- `tgkill`, `tkill`, `pthread_kill`, `rt_tgsigqueueinfo` and a POSIX timer's
  `SIGEV_THREAD_ID` reached the process, not the thread. For glibc's
  `SIGEV_THREAD` timers that meant the timer's signal (32, glibc's
  `SIGTIMER`) could be taken by a thread that had not blocked it -- whose
  default action for 32 ends the program -- instead of the helper thread
  waiting for it in `sigtimedwait`.
- `sigaltstack` was per process (native).
- `/proc/<pid>/status`'s `SigPnd` (the thread's own pending set) was always 0.

**The fix:** `proc::signal::SignalState` keeps the process's shared queue and
a `ThreadSignals` per thread -- blocked mask, `sigsuspend`'s saved mask,
alternate stack, and a queue of its own -- made when the thread starts
(`on_thread_start`, from its creator's mask) and dropped when it ends
(`on_thread_exit`, with what was sent to it alone). A thread takes its own
queue first, then the process's; a process-directed signal counts as blocked
only while every thread blocks it. `tkill`, `tgkill`, `rt_tgsigqueueinfo`,
the native `SYS_SIGNAL_TGKILL` and `SIGEV_THREAD_ID` timers post to the
thread's queue and wake only it; `rt_sigprocmask` and `SYS_SIGNAL_MASK` set
the calling thread's mask. Tested by `signal::self_test`'s
`test_per_thread_state`, `posix_timer::self_test`'s `SIGEV_THREAD_ID` group
and the ring-3 `spawn::self_test_linux_thread_signals` (a real second thread;
the same program passes on Linux 6.6).
