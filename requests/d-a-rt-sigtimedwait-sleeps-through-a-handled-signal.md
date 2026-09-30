# D → A: a Linux program's `rt_sigtimedwait` sleeps through a signal it has a handler for, where Linux ends it with `EINTR`

**Status:** open — for lane A; nothing else needed first.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-30

## In short

A Linux program waiting in `sigtimedwait` (or `sigwaitinfo`, or
`sigwait`) for one set of signals, which is sent a signal *outside* that
set that it has a handler for, should have the wait end at once with
`EINTR` and the handler run -- whether or not the handler asked for
`SA_RESTART`. Here the wait goes on until its timeout, or for ever without
one, and the handler runs only then. Python's `signal.sigtimedwait`
interrupted by `^C` is the everyday case: on Linux `KeyboardInterrupt`
arrives at once.

Found reading your `sys_rt_sigtimedwait` while doing libc's native
`sigtimedwait` (lane D's 115), not by running a program; the reading is
below so you can check it.

## Why, as far as I can read it

`sys_rt_sigtimedwait` (`kernel/src/syscall/linux.rs`) registers its task
with `register_signalfd_waiter(caller, task, mask)` -- the set it waits
for -- and parks. A post goes through `set_pending_info`, which wakes only
`take_matching_signalfd_waiters(pid, bit)`, the waiters whose mask holds the
posted signal. A signal outside the set therefore wakes nothing, and if
something else wakes the task, the loop takes nothing, re-registers and
parks again: there is no path out with `EINTR`.

## What Linux does

glibc 2.39 on Linux, measured (`posix/src/interrupt_oracle.txt`, the five
`sigtimedwait` lines; `posix/tools/oracle/interrupt_harness.py` made them):
a handler for another signal ends the wait at once with `EINTR`, with
`SA_RESTART` or without; a signal that runs no handler -- ignored, or
ignored by default -- does not end it. In the kernel's own terms
(`do_sigtimedwait`): the set is unblocked while the task sleeps, the sleep
is interruptible, and if it ends with no signal of the set to take, the
answer is `-EINTR` rather than `-EAGAIN`.

## A shape for the fix

Register the waiter for the set **and** for the signals deliverable to a
handler (`mask | !blocked`, less those whose Linux disposition is
`SIG_IGN` or a default of ignore), and after a wake with none of the set to
take, answer `-EINTR` if one of those is pending -- the delivery on the way
out runs its handler. Linux never restarts `rt_sigtimedwait`, so no restart
sentinel. `rt_sigsuspend` and `pause` already register the deliverable
mask; this is the same move.

## What lane D has on its side

Nothing for the native ABI: libc's native `sigtimedwait` takes its signal
from the trampoline's dispatch and ends on a futex wait the kernel already
interrupts (design-decisions §1158). This is only the Linux ABI.
