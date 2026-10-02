## D-LINUX-RT-SIGTIMEDWAIT-SLEEPS-THROUGH-A-HANDLED-SIGNAL — a Linux program in `sigtimedwait` is not woken by a signal outside its set that it has a handler for; Linux ends the wait with `EINTR` at once (lane D, 2026-09-30) — **Status: OPEN (waiting on lane A)**

**In short:** a Linux program (glibc, run through the Linux ABI) that waits
in `sigtimedwait` for one set of signals, and is sent another it has a
handler for, should see the wait end at once with `EINTR` and the handler
run -- Python's `signal.sigtimedwait` interrupted by `^C`, for one. Here
the wait goes on to its timeout, or for ever, and only then does the
handler run. Found by reading the kernel, not by running a program.

**Why:** `sys_rt_sigtimedwait` (`kernel/src/syscall/linux.rs`) parks as a
signal waiter for its set only, and `set_pending_info` wakes only waiters
whose mask holds the posted signal; the loop has no way out with `EINTR`.
glibc's answers (`posix/src/interrupt_oracle.txt`, "sigtimedwait"): a
handler ends the wait, `SA_RESTART` or not; a signal that runs no handler
does not.

**The proper fix:** park for the set and for the signals a handler would
take, and answer `-EINTR` when one of those, not one of the set, is what
ended the sleep -- requested of lane A in
`requests/d-a-rt-sigtimedwait-sleeps-through-a-handled-signal.md`. The
native `sigtimedwait` is libc's and does not have the fault
(design-decisions §1158).

**Where:** `kernel/src/syscall/linux.rs` (`sys_rt_sigtimedwait`).
