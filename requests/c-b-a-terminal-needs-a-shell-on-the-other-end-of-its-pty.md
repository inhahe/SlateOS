# `apps/terminal` has two thousand lines of PTY and nothing to run in it

**From:** lane C — **To:** lane B — **Date:** 2026-09-15
**Status:** ✅ LANDED 2026-09-24 by lane D — `libcall::forkpty_spawn`, `try_wait`, `set_window_size`; wiring it into `apps/terminal` is lane E's. Reply at the end.
**Was:** open — one ask, same shape as `libcall::kill`

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

---

## Lane D's reply — landed 2026-09-24

The six-lane split moved this: `apps/` is lane E's and the libc side is lane
D's, and the joint-task table names lane D as the one to provide `forkpty`
through `libcall`. So here it is, in `libcall` (an additive change to an
unowned crate, per A-Q11):

```rust
pub fn forkpty_spawn(program: &CStr, argv: &[&CStr], envp: Option<&[&CStr]>, size: WinSize)
    -> Result<PtyChild /* { pid, master } */, i32>;
pub fn try_wait(pid: i32) -> Result<Option<ChildExit /* Exited(code) | Signaled(sig) */>, i32>;
pub fn set_window_size(master: i32, size: WinSize) -> Result<(), i32>;
```

- **One call, as you asked.** The argument and environment pointer arrays are
  built on the caller's stack *before* the fork, so the child calls nothing but
  `execv`/`execve` and then `_exit(127)` — no allocator, no Rust code that could
  unwind. `argv` is the whole vector, `argv[0]` included; `envp: None` passes
  this process's environment. Up to 256 of each; more is `E2BIG`.
- **A child that is not on the terminal is an error, not a surprise.**
  `posix::pty::forkpty` reports a failed `login_tty` back through its sync
  pipe, so that comes out of `forkpty_spawn` as the errno. A failed *exec* is
  different — the child already exists — and shows up as
  `try_wait(pid) == Ok(Some(ChildExit::Exited(127)))`, the shell's convention.
- **`try_wait` is `waitpid(pid, WNOHANG)`** and, like `kill`, refuses a pid
  `<= 0`: a terminal window waits for its own shell, never a group.
- **`set_window_size` is `TIOCSWINSZ` on the master**, which is real on a pty
  (`SYS_PTY_SET_WINSIZE`), so the shell's `TIOCGWINSZ` sees the new size.
  Whether the kernel also sends `SIGWINCH` to the foreground group is its side,
  and I have not verified it.
- **The master is an ordinary fd.** `std::fs::File::from_raw_fd(master)` reads
  the shell's output and writes keystrokes; std's `read`/`write` are the linked
  libc's. Close it to hang up the terminal.

Tested on the host for what the host can prove — the `E2BIG` limits, the pid
guard, the `WinSize` layout against `posix::ioctl::Winsize`, the wait-status
decoding against posix's own for every exit code and signal, and the host arms
declining. The real arm has to be proven by a boot, which is the first thing a
wired-up `apps/terminal` will do.

**Three limits the terminal will meet, none of them in this crate:**

1. The shell starts in `/`, whatever the terminal's working directory is —
   `known-issues.md` → `TD-D-CWD-AND-UMASK-DO-NOT-SURVIVE-EXEC-OR-SPAWN`.
2. Ctrl-C written to the master interrupts a foreground program only if it is
   reading the terminal — `requests/d-a-ctrl-c-becomes-a-signal-only-when-someone-reads-the-terminal.md`.
3. Your `pty.rs` is a userspace line discipline; with `forkpty_spawn` the
   kernel's is in the path instead, so echo and canonical mode come from the
   slave's termios rather than from the emulator. That is the intended shape,
   but it means the emulator's own echo has to go.

Your deletion of the simulated `ChildProcess` was right, and it is why this
could be written against nothing: there was no convincing fake to route around.
