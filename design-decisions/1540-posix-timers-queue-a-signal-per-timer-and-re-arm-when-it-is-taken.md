## 1540. POSIX timers queue a signal per timer and re-arm when it is taken

**Date:** 2026-10-07 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** a program can now ask the kernel for timers that send it a
signal when they go off (`timer_create`, `timer_settime` and the rest), from
both kinds of program: Linux ones through the Linux system calls, native
ones through one new call (`SYS_POSIX_TIMER`) that lane D's C library passes
straight through. Until now the calls checked their arguments and then said
"not implemented", so GNU `timeout` fell back to `alarm` and nothing else
could have more than one timer. The behaviour is Linux's, measured on a
Linux 6.6 machine (`build/ptimer_oracle.c`, and the ring-3 test itself, which
passes there), except in two corners where Linux 6.13 changed it and this
follows 6.13. The dumpable flag got its native call too (`SYS_PROCESS_DUMPABLE`).

**How it works** (`proc::posix_timer`): an armed timer has one `hrtimer`
pending; its expiry, in the timer interrupt, queues the timer's signal; a
periodic timer is re-armed only when that signal is taken from the queue.
The timers live in a slab the interrupt indexes directly, and each process's
signal queue keeps room for one entry per timer it owns, reserved when the
timer is created, so the interrupt never allocates.

**Choice 1 -- a queue entry per timer, not the one record per signal.**
The signal layer kept one record per signal number: a second `kill` of a
pending signal is merged into it, as Linux does for the standard signals.
Timers cannot share that record.

| | an entry per timer (chosen) | the shared record |
|---|---|---|
| two timers on one signal | both deliver, as on Linux | the second expiry is lost -- and a periodic timer waiting to be re-armed by its signal stops for good |
| knowing which timer a taken signal was | the entry says | only `si_timerid`, and only if it was not overwritten |
| cost | a `Vec` of entries per process, reserved at `timer_create`; every place that takes a signal goes through one helper | none |

glibc's `SIGEV_THREAD` puts every such timer on one signal, so sharing is the
normal case, not a corner.

**Choice 2 -- re-arm when the signal is taken (Linux's model), not a free-running
periodic timer.** A periodic `hrtimer` could keep firing and count the
expiries that found the signal still pending. That costs an interrupt per
interval for as long as the signal stays blocked -- a 1 µs timer blocked for
an hour is 3.6 billion interrupts -- where Linux's model costs one per
delivered signal: the expiry queues the signal and stops; taking the signal
moves the expiry past now by whole intervals, which are the overrun, and
re-arms. That needs the signal layer to tell the timer when its entry is
taken, by delivery, `sigtimedwait`, `signalfd`, or a discard, and it does,
from every path, outside its lock.

**Choice 3 -- a timer re-set or deleted while its signal is queued does not
deliver it (Linux 6.13), rather than delivering it anyway (6.6).** POSIX
leaves it unspecified. The stale signal of a deleted timer carries a value
that may point at freed memory -- glibc's `SIGEV_THREAD` helper has to check
for exactly that -- and 6.13 stopped sending it. Re-setting a timer also
restarts both overrun counts (6.13), where 6.6 kept a count that could belong
to the old arming.

**Choice 4 -- a periodic timer whose signal is ignored re-arms itself at
least a tick out (Linux 6.6), rather than parking until the signal is no
longer ignored (6.13).** 6.13's way saves the ticks, but needs a hook from
every change of a signal's action back into the timers. 6.6's needs none, and
both give the same overrun count when the signal is let through again.

**Choice 5 -- the CPU-time clocks answer `EOPNOTSUPP`.** Linux times them.
This kernel keeps no per-task CPU time to fire on (and `clock_gettime` of them
reads wall time -- known-issues `A-CPU-TIME-CLOCKS-READ-WALL-TIME-AND-CANNOT-BE-TIMED`).
Succeeding with a timer that would fire on wall time would be wrong in a way
nobody could see; `EOPNOTSUPP` is what Linux answers for a clock it cannot
time, and the id is still checked first (`EINVAL` for a process or thread that
does not exist), as Linux would.

**Choice 6 -- `SIGEV_THREAD_ID` is accepted and its thread checked.** At
first the signal went to the process, as every signal then did; the same day
signals gained per-thread state (§1542), and a `SIGEV_THREAD_ID` timer's
signal now waits on its thread's own queue. Refusing `SIGEV_THREAD_ID` would
have made glibc's `SIGEV_THREAD` timers fail outright.

**Choice 7 -- one native call with an operation argument, taking Linux's
structures and answering Linux's errnos.** Lane D asked for either. One call
with Linux's `struct sigevent` and `struct itimerspec` lets the C library
pass the user's arguments straight through, as it does for `madvise` (§1538),
and keeps the five operations' checks in one place for both ABIs.

**Found on the way, and fixed:** `rt_sigtimedwait` and `signalfd` reported
only the signal number, never its code, sender or value -- which glibc's
`SIGEV_THREAD` helper needs to find its timer -- and a Linux handler's
`siginfo` lost the value of everything but `sigqueue`, a `SIGCHLD`'s
`si_status` included. Both now report the whole record. And a signal frame,
native or Linux, was built right below the interrupted stack pointer, over
the 128-byte red zone that the interrupted function may still be using;
it now goes below it, as Linux's `get_sigframe` does.
