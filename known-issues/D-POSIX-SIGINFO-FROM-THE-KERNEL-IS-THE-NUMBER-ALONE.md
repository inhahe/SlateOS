## D-POSIX-SIGINFO-FROM-THE-KERNEL-IS-THE-NUMBER-ALONE — for a signal the kernel delivers, a native `siginfo_t` says `SI_USER` and names no sender: the kernel's record of the signal does not reach the trampoline (lane D, 2026-09-30) — **Status: OPEN (waiting on lane A)**

**In short:** a program that asks who sent a signal -- in an `SA_SIGINFO`
handler, or from `sigwaitinfo` -- is told "a user process, pid 0" for every
signal that came from outside it: another process's `kill`, a child's exit,
a timer. A `SIGCHLD` handler that reaps the child `si_pid` names, a daemon
that logs who sent it `SIGTERM`, a POSIX-timer handler that finds its timer
through `si_value`, all get nothing to go on. Signals the program raises
itself are described correctly.

**Why:** the kernel keeps the record for every pending signal
(`kernel/src/proc/signal.rs`, `SigInfo`: code, sender pid and uid, value)
and hands it to Linux-ABI programs, but the native path takes the signal
with `take_deliverable`, which drops it, and the native frame
(`SignalContext`) carries the number alone.

**The proper fix:** a native frame that carries the record, opted into at
`SYS_SIGNAL_REGISTER` so libc and kernel need not change together; libc then
reads it into the `siginfo_t` (`siginfo_for`) and the waits' answer.
Requested of lane A, with a proposed layout, in
`requests/d-a-put-each-signal-s-siginfo-in-the-native-frame.md`; also that
`SIGCHLD`'s record carry the exit status, and a native call that posts a
signal with a value, which `sigqueue` -- a stub answering `ENOSYS` -- needs
to send one.

**Where:** `posix/src/signal.rs` (`siginfo_for`, `hand_to_a_waiter`);
`kernel/src/syscall/handlers.rs` (`deliver_pending_signal`).
