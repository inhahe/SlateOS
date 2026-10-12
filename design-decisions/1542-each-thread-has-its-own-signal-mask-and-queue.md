## 1542. Each thread has its own signal mask and queue

**Date:** 2026-10-07 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** a program with several threads can now block a signal in one
thread without blocking it in the others, and can send a signal to one thread
rather than to the whole program -- as on Linux, and as glibc's threads, its
timer helper and `pthread_kill` assume. Until now the kernel kept one blocked
mask and one list of pending signals per program: one thread blocking `SIGINT`
blocked it for all, and a signal meant for one thread could be taken by any.
Single-threaded programs see no difference.

**The model** (`proc::signal`): a process's `SignalState` holds the shared
queue (signals sent to the process), the per-process parts of a disposition
(the ignored set, the trampoline, which signals want the alternate stack), and
a `ThreadSignals` per thread: blocked mask, `sigsuspend`'s saved mask,
alternate stack, and a queue of signals sent to it alone. A thread's state is
made when it starts, from its creator's mask (Linux's `clone`), and dropped
when it ends. A thread takes its own queue first, then the shared one
(Linux's `dequeue_signal`); a process-directed signal is "blocked" only while
every thread blocks it; a thread-directed one wakes only its thread.

**Choice 1 -- the calling thread is implicit, not an argument.** Nearly every
mask and pending query in the kernel is made by a thread about itself -- the
syscall-return checkpoint, every interruptible wait, `sigprocmask`,
`sigtimedwait` -- as Linux's code uses `current`. So `signal::blocked(pid)`,
`set_blocked`, `pending`, `has_pending_in_mask` and the dequeue functions
answer for the calling thread when it is one of `pid`'s, and the hundred-odd
callers did not change. A caller from outside the process (the kernel, a
self-test) reads and sets the process's **template**: the state a process's
first thread starts with, which spawn, fork and exec set. The alternative,
an explicit thread argument everywhere, would have touched every wait loop for
no difference in behaviour; the explicit forms exist (`*_as`) for the tests.

**Choice 2 -- every thread gets state of its own at start, rather than only
once it changes something.** Lazily created state would leave a thread that
never touched its mask reading the template -- correct only until another
thread's state diverged from it. Making it at start (one hook in the one place
threads are registered) keeps "a thread reads its own state" true without
exceptions; `/proc/<pid>` reads the oldest thread's.

**Choice 3 -- POSIX timer entries stay in the process's one timer queue,
with a target.** A `SIGEV_THREAD_ID` timer's entry carries its thread, and its
signal's bit lives in that thread's queue. Keeping one `Vec` per process keeps
the reservation that lets a timer interrupt queue without allocating (§1540)
one per process too.

**What changes for native programs:** `SYS_SIGNAL_MASK` now sets the calling
thread's mask (Linux's `sigprocmask` does too; POSIX leaves it unspecified for
a multi-threaded process), `SYS_SIGNAL_PENDING` reports the calling thread's
view, `SYS_SIGNAL_ALTSTACK` the calling thread's stack, and `SYS_SIGNAL_TGKILL`
aims at its thread -- so lane D's `pthread_kill` can now use it, and
`pthread_sigmask` is the per-thread call it was meant to be
(`requests/a-d-signal-masks-are-per-thread-and-tgkill-aims-at-the-thread.md`).
