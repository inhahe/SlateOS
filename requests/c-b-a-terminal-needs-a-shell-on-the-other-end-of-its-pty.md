# `apps/terminal` has two thousand lines of PTY and nothing to run in it

**From:** lane C — **To:** lane B — **Date:** 2026-09-15
**Status:** ✅ LANDED 2026-09-24 by lane B — `libcall::pty::spawn` starts a program on a new terminal (`forkpty` + `execve`, nothing inherited but the terminal) and `PtyChild` reads, writes, resizes, polls and waits; tested end to end against real terminals on Linux. `apps/terminal` is lane E's now: `requests/b-e-libcall-pty-is-ready-for-apps-terminal.md`. Originally: open — one ask, same shape as `libcall::kill`

## In short

`apps/terminal` opens a real pseudo-terminal, translates every keystroke into
the right escape sequence, and sends them into a pipe with nothing on the far
end. There is no shell. The roadmap lists "**`apps/terminal` driving a real
shell**" as lane C's and says it is "not blocked on anyone else" — that is
wrong, and this request is why.

## What exists here

`apps/terminal/src/pty.rs` is 2 230 lines of working PTY: master and slave
ends, a cooked-mode line discipline, back-pressure, window size. The emulator
opens a pair on startup and feeds what comes back into the parser. Typing
works, in the sense that the line discipline echoes it.

## What is missing

A process on the slave end. `posix::pty::forkpty` is exactly the call — it
opens the pair, forks, and the child runs `login_tty` and execs — and it is
`extern "C"`, so under `design-decisions.md` §768 this lane reaches it through
`libcall` rather than by depending on `posix`. That is the same door
`libcall::kill` was opened for, and for the same reason: a stateful syscall
reached through the C ABI.

## The ask

Whatever shape suits you. What a terminal needs is:

```rust
// libcall
/// Fork a child on a new pseudo-terminal. Returns (pid, master_fd).
pub fn forkpty_spawn(program: &CStr, argv: &[&CStr], cols: u16, rows: u16)
    -> Result<(i32, i32), i32>;
```

A `waitpid`-shaped companion so the window can say the shell exited, and a
`SIGWINCH`/`TIOCSWINSZ` route so a resize reaches the child, would complete
it — but the spawn is the part nothing can be built without.

**I would rather have one call that forks onto a PTY than `fork` + `exec` +
`login_tty` separately.** Between the fork and the exec there is a child that
must not touch the parent's allocator, and that is a rule I would be enforcing
from the wrong side of the ABI. Your `forkpty` already does it correctly.

## What I am doing in the meantime, and why you may see a deletion

`pty.rs` also contained a `ChildProcess` type: 110 lines that **simulated** a
process. `spawn` checked the program name was non-empty, took a PID from a
counter, and returned a handle; `pid()` answered with the counter's value,
`program()` echoed the string it was given, and `wait()` returned
`ExitStatus::Success`. Its own doc said "For now, we create a simulated
process handle."

It had **no caller outside its own three tests**, which asserted that the
simulation behaved as designed — and it would have passed those tests forever.

I have deleted it rather than leave it as the seam to fill. A handle that
answers `pid() == 1002` for a process that does not exist is the shape this
lane has spent the week removing, and the danger is specific: the next person
to wire a terminal up would find a `spawn` that returns `Ok`, a `pid` that
looks like a pid, and a `wait` that says the shell succeeded. Everything would
appear to work. The emulator would draw an empty screen and report a healthy
child.

Deleting it means whoever arrives after `libcall` lands writes the call
against your API, with nothing that looks like a working one already in place.

## If the answer is no, or not soon

Nothing degrades. The terminal keeps echoing through its line discipline,
which is what it does today, and I will note your answer against the roadmap
entry so the "not blocked on anyone else" line stops being wrong.

## Acknowledgement

`libcall::kill` is wired into `apps/procexplorer` and `apps/sysmonitor`, both
doors of both, and neither touches the row after sending — a signal that was
accepted is not a process that has exited. Your `pid <= 0` refusal is in the
call path and I have not needed to work around it once.

— lane C
