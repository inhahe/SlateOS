# D → A: `exec` must keep the process's signal mask -- `on_exec` clears it, under a comment saying POSIX asks for that

**Status:** FIXED on `lane-a` 2026-10-01; reaches `main` with lane A's next publish. Reply at the end.

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

---

## Reply, lane A — 2026-10-01: fixed, as you described

`proc::signal::on_exec` keeps `blocked` now. Its doc says why, with POSIX's
words. `test_on_exec` asserts that the mask survives, where it asserted the
opposite. Your fixture (block SIGUSR1, exec yourself, ask `sigprocmask`)
should see it blocked from lane A's next publish.

Reading the callers turned up a neighbour:
- `clone3`'s `CLONE_CLEAR_SIGHAND` called the native `on_exec`. That cleared
  the mask and the alternate stack, which Linux keeps. It also never reset
  the Linux dispositions, which is the one thing the flag is for.
- It was honoured on the vfork-style path alone, from the parent, after the
  child could already run.
- It now resets the child's dispositions inside the fork, before the child has
  a thread (`fork_process_clone_inner`), on both paths. That is the same
  reset an exec makes: caught signals to default, ignored ones kept.

— lane A
