## §115 — Terminal-access job control: one pure decision, two ABIs, and the disposition the kernel cannot see

**Date:** 2026-08-12

**Decided by:** Claude (autonomous). Mine to revisit; the operator may overrule.

**Context.** §113 and §114 both closed with the same sentence: the foreground
process group is real, the terminal's modes are real, but nothing *enforces*
the distinction. A background process could read the keystrokes meant for the
foreground job, and could reconfigure or seize the terminal out from under it.
`pcb::ctty_is_background` had existed since §113 and had no callers. This is
the last of the three pieces `todo.txt` recorded as deferred (the other two —
Linux's `TIOCSCTTY` steal argument and per-process `TIOCNOTTY` — are blocked
on a credential model and on a second terminal existing, respectively).

Linux's `__tty_check_change()` (`drivers/tty/tty_io.c`) is the model, and it
has two asymmetries that are easy to get backwards:

```c
if (tty_pgrp && pgrp != tty_pgrp) {
    if (is_ignored(sig))            { if (sig == SIGTTIN) ret = -EIO; }
    else if (is_current_pgrp_orphaned())  ret = -EIO;
    else { kill_pgrp(pgrp, sig, 1); ret = -ERESTARTSYS; }
}
```

First, the signal goes to the **caller's own** group, not the terminal's
foreground group. Second, when the signal is ignored or blocked, a *read*
fails `EIO` while a *write* is let through — because the point of refusing the
read is to keep terminal input away from a background job, and there is no
equivalent harm in output.

**Decision 1 — the policy is a pure function; only a thin wrapper has
effects.** `handlers::tty_job_control_decide(pid, sig) -> TtyAccessDecision`
(`Allow` / `Signal(sig)` / `Fail(err)`) contains all of the logic;
`tty_job_control_check(sig)` resolves the caller, calls it, fans the signal
out over `pcb::pids_in_group`, and returns the restart sentinel.

The split is not aesthetic. The effectful form asks about *the calling
process*, and the kernel self-test task owns no process at all — `caller_pid()`
returns `None` — so a self-test can never put itself in the background of a
terminal in order to be checked. Every branch of this policy would have been
unreachable from `cargo test` and from the kernel's own self-tests, leaving
only the ring-3 fixture, which cannot construct an orphaned group on demand.
With the decision extracted, `dispatch.rs::test_dispatch_tty_job_control`
builds shell + job + orphan states directly out of `pcb` and asserts all four
cases.

**Decision 2 — the blocked-signal check is a liveness requirement, not a
nicety.** It is tempting to read the `is_ignored` arm as an errno refinement.
It is not: a *blocked* `SIGTTIN` stays pending and undeliverable, so posting it
and returning `ERESTARTSYS` would restart the read, re-check, post again — an
unkillable spin inside the kernel. The kernel owns the blocked mask for both
ABIs, so this half is always exact and is checked first.

**Decision 3 — the kernel deliberately cannot see a native-ABI `SIG_IGN`, and
that is documented rather than approximated.** `signal_ignored_or_blocked`
answers from three sources: the blocked mask (kernel-owned, exact); "no
trampoline registered", in which case the kernel's own default-action table
*is* the disposition (exact); and an explicit `SIG_IGN`, which lives in
`linux.rs`'s private `linux_sigaction_table` and is therefore visible only for
`AbiMode::Linux`. This is not an oversight but the standing architecture:
`proc/signal.rs` states that userspace owns the native per-signal disposition
table, which is exactly why `SYS_SIGNAL_STOP_SELF` exists — userspace *reports*
an already-resolved disposition rather than asking the kernel to re-derive one.

The alternative — inventing a native `sigaction` table in the kernel now, purely
so this one predicate could consult it — would have duplicated state that
userspace already owns, which is the bug shape §113 and §114 were spent
removing. The consequence is recorded instead: a job-control shell on our
native libc must **block** `SIGTTOU` around `tcsetpgrp` rather than ignore it
(bash ignores it). `todo.txt` item 1a states the proper fix and its trigger,
and the ring-3 fixture demonstrates the blocking form so the limitation is
executable documentation rather than a comment.

**Decision 4 — de-duplicate the enforcement points instead of adding the
policy twice.** The Linux shim's `TCSETS`/`TCSETSW`/`TCSETSF` and `TIOCSPGRP`
arms had their own copies of the `copy_from_user` + `tty::set_termios` and
`ctty_set_fg_pgrp` bodies. Adding the gate to both copies would have been two
places to forget it. They now delegate to shared
`handlers::tty_set_termios_from_user` / `tty_set_pgrp_checked` returning a
`TtyCtlOutcome`, the same shape `dispatch_console_read` already used for reads
in §114.

Note the `TOSTOP` asymmetry, which mirrors `n_tty`: the write gate is
conditional on the new `lflag::TOSTOP` (off by default, as on Linux), while the
read gate and the terminal-control gate (`tcsetattr`, `tcsetpgrp`) always
apply. `TOSTOP` governs background *output* only.

**Two bugs this uncovered.**

- **`linux_from_native` was flattening restart sentinels to `EINVAL`.** It maps
  any negative it cannot decode as a `KernelError`, and `restart_result`'s
  `-512` is exactly that. Latent until `sys_console_write` grew a `SIGTTOU`
  gate, since `dispatch_write` routes console writes through it. The guard
  added there is load-bearing, not defensive.
- **`posix`'s `ctty_errno` mapped `IoError` and `Interrupted` to `ENOTTY`.**
  Its fallback arm is `ENOTTY` on the reasoning that it is the conservative
  reading — true for an unknown failure, actively wrong for these two, which
  job control had just made reachable. `ENOTTY` tells a shell "you are not
  interactive, skip job control", i.e. the exact opposite lesson to draw from
  having just been stopped for touching the terminal. Both now map explicitly.
  This is the same class of defect §114 found in `errno::native` (missing
  `INTERRUPTED`/`DEADLOCK`) — an error table that was complete for the set of
  errors reachable when it was written.

**Consequences.**

- `pcb`'s orphan predicate was split. The existing function conflated "is
  orphaned" with "has a stopped member" (the `SIGHUP`+`SIGCONT` trigger);
  job control needs the plain form. Both now derive from one locked pass
  (`pgrp_orphan_state`), so the two callers cannot see the group in two
  different states.
- `proc/signal.rs`'s private `signal_bit` became public and absorbed a second,
  freshly-written copy of the same `sig - 1` shift, plus an open-coded
  `1 << ((sig - 1) & 63)` in `emit_linux_rt_frame`. That `& 63` was a real
  latent bug: an out-of-range signal number would have blocked some *other*
  signal for the duration of a handler, where `signal_bit` yields `None`.
- A native process with **no** trampoline gets full POSIX behaviour on the
  restart path — `entry.rs` runs `deliver_pending_signal` (which performs the
  stop inline and returns after `SIGCONT`) before `resolve_syscall_restart`
  rewinds `RIP`. A native process **with** a trampoline sees `EINTR` instead,
  because our native ABI has no `SA_RESTART` for a handler to request.
- The ring-3 fixture gained checks 72–86, which run in the one window where it
  is genuinely background — after handing the terminal to its child's group.
  It cannot be stopped there: the kernel spawns it with parent 0, so its group
  is orphaned and it gets `EIO`. That made the fixture's old check 73 (`EPERM`
  for foregrounding a reaped child's empty group) unreachable, which is
  correct — job control fires first — so the check was kept and moved behind a
  `sigprocmask(SIG_BLOCK, SIGTTOU)`, which is what a real shell must do anyway.
