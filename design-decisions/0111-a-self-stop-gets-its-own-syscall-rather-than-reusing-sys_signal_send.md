## §111 — A self-stop gets its own syscall rather than reusing `SYS_SIGNAL_SEND`

**Date:** 2026-08-12

**Decided by:** Claude (autonomous). Mine to revisit; the operator may overrule.

**Context.** The `Stop` default action (SIGSTOP/SIGTSTP/SIGTTIN/SIGTTOU) was
the last default action `posix` could not carry out. Its code said *"We have no
kernel suspend mechanism; report ENOSYS"* — and that had been false for weeks:
`TaskState::Suspended`, `JobControlEvent`, `stop_process_for_signal` and
`continue_process` all existed and were driven by the *cross-process* signal
path. What was missing was only a way for a process to ask for its own stop.
That stale comment is the same expired-rationale defect as Q41's §72 blocker
and the `nnp_guard()` doc; it is recorded here because the fix is one line of
code and the *finding* is the valuable part.

**The obvious implementation is wrong.** `kill(getpid(), SIGTSTP)` looks like
it should work, and for SIGSTOP it nearly does. But `classify_post_info` tests
SIGSTOP and SIGCONT explicitly *before* it tests `has_trampoline`, so the three
**catchable** stop signals fall through to the trampoline branch — and a native
process always has a trampoline registered, from `init_signals()`. A self-sent
SIGTSTP would therefore be marked pending for handler delivery, the trampoline
would run `dispatch_self_signal`, and that is the function that just resolved
it to `SIG_DFL` and asked for the stop. The result is an infinite delivery
loop, not a stop. Reordering `classify_post_info` to test the stop signals
first was rejected: that check is what makes a *cross-process* SIGTSTP
catchable, which is required — a shell must be able to trap Ctrl-Z.

**Decision.** Add `SYS_SIGNAL_STOP_SELF` (1062), taking only a signal number,
validated to 19..=22, which calls `stop_process_for_signal(pid, sig, Some(current))`
directly. Two properties follow that a send-based version could not have:

- **The wait status names the right signal.** The parent's `WSTOPSIG` reports
  the signal that actually stopped the child, so a shell's Ctrl-Z shows
  SIGTSTP rather than being flattened to SIGSTOP.
- **It grants no authority.** There is no pid argument and no capability check,
  because self-only is a property of the *signature*, not of a runtime test.
  Cross-process stops stay on `SYS_SIGNAL_SEND` behind `CAP_KILL`. Adding a pid
  argument here — the tempting generalisation — would turn it into a cheaper,
  un-capability-checked route to exactly that authority, so it is deliberately
  absent.

*Against:* it is one more syscall number for something POSIX expresses with
`kill()`, and it splits "stop" across two entry points. *For:* the alternative
is not "reuse `kill`" but "reuse `kill` **and** reorder the classifier so that
cross-process SIGTSTP stops being catchable" — that is a worse trade, and the
loop it would create is silent rather than diagnosable.

**Testing note.** The kernel self-test covers only the rejection path. A valid
stop signal suspends every thread of the caller and returns only on SIGCONT, so
issuing one from the boot self-test task would park the boot thread with nobody
left to resume it. The argument gate runs before any process lookup, so the rejection
cases short-circuit safely. The accept path needs a second process to observe
the stop and send the SIGCONT, so it belongs in a ring-3 test.
