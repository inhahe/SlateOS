# D → A: a native call that takes a pending signal of a set, with its record, as `rt_sigtimedwait` does for Linux programs

**Filed:** 2026-10-17 by lane D. **For:** lane A (`kernel/src/proc/signal.rs`,
`kernel/src/syscall/`). **Status:** OPEN. **Priority:** normal -- the
library's `sigwait` works meanwhile; what it loses is rare, but when it is
a timer's expiry, that expiry's notification never runs.

## In short

`sigwait`, `sigtimedwait` and `sigwaitinfo` take a signal without running
its handler. A Linux program here gets that from your `rt_sigtimedwait`,
which takes the signal off the pending set with its record
(`take_pending_info_in_mask`). A native program has no such call, so the C
library takes the signal the long way round (design-decisions §1158): it
lets the kernel deliver the signal to the trampoline, and the dispatch
gives it to the waiting thread instead of to a handler.

That works for one signal. When a second signal of the set comes a moment
after the first -- before the library has closed the set in the thread's
kernel mask -- the kernel delivers that one too. By then the waiter
already holds a signal, and the library cannot give the second back to
the kernel as it was: no native call queues a signal to a thread with a
record. So it re-sends the bare number to its own process, and the
second signal loses its sender, its code, its value and its thread.

For the timer thread behind `SIGEV_THREAD` timers, which takes every
expiry this way, the value is what names the timer. That expiry's
notification never runs.

Round 205 narrowed the window: the dispatch now closes the set the moment
it hands a signal over, so a second signal already pending stays with
you. Only a signal posted in that moment, and delivered from an interrupt
taken in it, is still lost. Closing the window entirely needs the signal
to stay pending until it is taken.

## Asked

A native call with `rt_sigtimedwait`'s semantics. Name and number are
yours; for example:

    signal_take(set: u64, timeout_ns: u64, info_out: *mut SignalInfoTail) -> sig

- **Takes** the lowest-numbered pending signal of `set` (`SIGKILL` and
  `SIGSTOP` dropped), from the calling thread's own queue first and then
  the process's, without delivering it. Any mask is ignored, as
  `take_pending_info_in_mask` ignores it.
- **The record** goes to `info_out` in the native frame's layout
  (`SignalInfoTail`, as the extended frame carries it). A timer's entry
  goes through its timer first, so a periodic timer re-arms and the
  overrun is stamped, as it does today.
- **Waits** until a signal of the set comes or the timeout passes. Make
  `u64::MAX` mean no timeout and 0 mean a single poll. A timeout that
  passes answers `TimedOut` (the library's `EAGAIN`).
- **Ends early** with `Interrupted` when a signal outside the set
  arrives that the calling thread's kernel mask lets through and the
  process does not ignore. The delivery on the way out then runs it, and
  the library's dispatch decides what it does. This is the native form of
  your `sigtimedwait_interrupters`: the kernel knows no handlers here, so
  every deliverable signal counts.
- A freeze ends it with `Interrupted` too, as yours does.

## What the library does with it

`sigtimedwait` and the timer thread take their signal with the call
directly. The table of waiters goes, with everything it carries:
`Acceptor`, `hand_to_a_waiter`, `extra`, the waiters' sets in the kernel
mask (`opened_by_waiters`, `kernel_mask_now`), and the fork-time reset.
`sigwaitinfo` then reports the full record, and no signal is lost or
re-sent. About 300 lines of `posix/src/signal.rs` go, and their tests
with them.

`known-issues/D-POSIX-SIGWAIT-CAN-LOSE-A-SECOND-SIGNALS-RECORD.md`
describes the loss.
