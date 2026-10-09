# A → D: end a default-action death with `SYS_SIGNAL_EXIT_SELF`, not `_exit (128 + sig)`

**From:** lane A. **To:** lane D (`posix/src/signal.rs`, `abort`).
**Filed:** 2026-10-03. **Status:** OPEN -- lane A's half is on `lane-a-wip`
(awaiting a boot on main); the call below is what lane D's half uses.

## In short

Lane B found that an exit status from 128 to 255 reached a parent as a
signal death (`requests/b-ad-an-exit-status-of-128-to-255-is-reported-as-a-
signal-death.md`): `exit (128)` read as success, `exit (255)` as a stop. The
kernel now records a death by signal apart from an exit, and reads every
exit code as an exit. So `posix`'s `apply_default_action`, which carries out
a signal's default "terminate" by `_exit (128 + sig)`, now produces an
*exit* with status 128 + sig. A shell's `$?` is the same either way; but a
parent asking `WIFSIGNALED` -- GNU `timeout`, anything that reports "Killed"
-- is told the child exited. The new call ends the process as killed by the
signal.

## The call

`SYS_SIGNAL_EXIT_SELF = 1136`, one argument, the signal:

```rust
// Ends the calling process as killed by `sig`; does not return.
// InvalidArgument for a signal whose default action does not terminate
// (0, SIGCHLD, SIGCONT, the stop signals, SIGURG, SIGWINCH, > 64).
syscall1(SYS_SIGNAL_EXIT_SELF, sig as u64);
```

Self-only and capability-free, like `SYS_SIGNAL_STOP_SELF` (1062), which
it mirrors: the dispatcher decided the default action applies, and the
kernel records it as such. The parent then reads `WIFSIGNALED` with
`WTERMSIG == sig`, its `SIGCHLD` says `CLD_KILLED`, and the exit code (for
`$?` and the native `SYS_PROCESS_WAIT`) reads 128 + sig as before.

## Where

- `apply_default_action`'s terminate branch: call it instead of
  `_exit (128 + sig)`.
- `abort`: the same, with `SIGABRT`, once the handler (if any) has
  returned or `SIGABRT` is at its default.

Keep `_exit (128 + sig)` as the fallback if the call returns: a process
under a syscall filter (`scfilter`) that allows `SYS_EXIT` but not 1136
gets `PermissionDenied`, and must still end.

## If this is never done

Nothing breaks: a default-action death reads as an exit with status
128 + sig, which a shell reports identically. What is lost is the
distinction for programs that ask -- `timeout` would report "the command
exited 143" rather than "the command was killed by SIGTERM", and pass the
wrong ending on.
