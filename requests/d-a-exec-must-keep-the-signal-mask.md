# D → A: `exec` must keep the process's signal mask -- `on_exec` clears it, under a comment saying POSIX asks for that

**Status:** open — for lane A; nothing else needed first.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-30

## In short

When a program replaces itself with another (`execve`), the new program is
supposed to start with the same signals blocked as the old one had. Shells
and supervisors rely on it: block `SIGCHLD` or `SIGINT` around a fork, exec
the command, and the command starts with them blocked until it says
otherwise. Here the new program always starts with nothing blocked, for
Linux programs and native ones alike.

## Where

`proc::signal::on_exec` (`kernel/src/proc/signal.rs`):

```rust
// Blocked mask is also reset on exec per POSIX.
state.blocked = 0;
```

POSIX says the opposite -- `execve`, "The new process image shall inherit
at least the following attributes from the calling process image: ...
Process signal mask (see sigprocmask())" -- and so does Linux, which keeps
`current->blocked` across `exec` untouched. What `exec` resets is the
*dispositions*: caught signals go back to default, ignored ones stay
ignored (the latter is `requests/d-a-ignored-signals-and-spawn-attributes-need-a-kernel-record.md`).
Pending signals, which the comment above it rightly keeps, are the other
attribute that survives.

## The fix

Drop the line; keep `blocked` as it is. `linux_sigaction_on_exec` already
does the disposition half for Linux programs.

## What lane D has done on its side

Native libc now reads the mask it inherited at start-up (`init_signals`,
`posix/src/signal.rs`) instead of assuming none. Today that reads 0, as
your `on_exec` leaves it, so nothing changes until this lands -- and then
the native side is right without a second change.

## How to see it

A fixture that blocks `SIGUSR1`, `exec`s itself with an argument, and in the
new image asks `sigprocmask` whether `SIGUSR1` is blocked: yes on Linux, no
here. Lane D will write it when this is in (it would fail today by design,
not by regression).
