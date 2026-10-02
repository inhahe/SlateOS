## D-POSIX-SIGWAIT-AND-SIGTIMEDWAIT-ARE-STUBS — `sigwait` sleeps a second and answers EINTR, `sigtimedwait` and `sigwaitinfo` answer EAGAIN at once; none takes a signal (lane D, 2026-09-30) — **Status: FIXED 2026-09-30 (`posix/src/signal.rs`)**

**In short:** a program that blocks a signal and waits for it with
`sigwait` -- the usual way a multithreaded server handles `SIGTERM` or
`SIGHUP`, on a thread of its own -- never gets it here. `sigwait` sleeps
for a second and returns `EINTR` without having taken anything, and
`sigtimedwait` and `sigwaitinfo` return -1 with `EAGAIN` at once. Their
comments still say the system delivers no signals, which stopped being true
when the trampoline arrived.

**Why it is not simply done:** the native kernel has no call that takes a
pending signal off the pending set without delivering it, and its signal
mask is the process's, not the thread's.

**The proper fix:** in the library, beside the trampoline's dispatch. A
thread in `sigwait` registers the set it accepts -- lock-free, since the
dispatch runs in signal context and must never wait on a lock the thread it
interrupted holds -- and lets the set through the mask. The dispatch, on
whichever thread the kernel delivers to, hands a signal of a registered set
to its waiter instead of running a handler, and wakes it with a futex wake;
the waiter blocks the set again. A handler for a signal outside the set ends
`sigtimedwait` with `EINTR`, `SA_RESTART` or not, and one that runs no handler
does not end it -- glibc's answers (`interrupt_oracle.txt`, "sigtimedwait").
The siginfo it can fill is the signal's number: the native frame carries no
more.

**Where:** `posix/src/signal.rs` (`sigwait`, `sigtimedwait`,
`sigwaitinfo`, `dispatch_self_signal`).

**Fixed 2026-09-30.** `sigtimedwait` takes a signal of its set: one pending,
at once, or the next to come, to whichever thread the kernel delivers it. A
waiting thread publishes its set in a lock-free table; the kernel's mask
lets the set through for as long as one waits; and the trampoline's
dispatch, on whichever thread the signal lands, hands it to the waiter
instead of running its disposition, and wakes it. The program's own mask --
what `sigprocmask` reports, and what a handler sets and restores -- is not
touched, so a handler ending on another thread mid-wait cannot close the set
again; the kernel's mask is recomputed from the two at every change
(design-decisions §1158). A handler for a signal outside the set ends the
wait with `EINTR`, `SA_RESTART` or not, and a signal that runs no handler
does not -- glibc's answers, replayed on the host
(`interrupt::tests::every_interruption_is_glibcs`, 145 lines). `sigwait`
begins again after one, as glibc's does, and `sigwaitinfo` is `sigtimedwait`
with no timeout. The siginfo carries the signal's number and nothing else:
the native frame brings no more. On the way, the dispatch stopped leaving a
blocked signal pending while the kernel's mask might still let it through --
it would have been delivered straight back into the same dispatch -- and a
child of `fork` forgets its parent's waiters. `services/ctest-eintr` gained
two ring-3 checks: `sigwait` taking a blocked signal its child sends, and
`sigtimedwait` ending for an `SA_RESTART` handler.
