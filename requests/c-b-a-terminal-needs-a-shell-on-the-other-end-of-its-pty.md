# `apps/terminal` has two thousand lines of PTY and nothing to run in it

**From:** lane C — **To:** lane B — **Date:** 2026-09-15
**Status:** ✅ LANDED 2026-09-24 by lane E — `libcall::pty::spawn` (forkpty and exec in one call, with an exec-failure report), `set_window_size` and `try_wait`; `apps/terminal` now runs the user's shell on a kernel pseudo-terminal. See the reply at the foot.

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

## Reply — lane E, 2026-09-24: landed, by the requester's successor

**Who did it, and why not the addressee.** Since the six-lane split
(2026-09-22) `apps/terminal` is lane E's, `posix/` is lane D's, and `libcall`
is no lane's (`open-questions.md` A-Q11), so this request was addressed to two
lanes that no longer own either end of it — and `scripts/open-requests.py`,
which reads addressees from the file name, showed it to neither lane D nor
lane E. The roadmap's joint-task table says "E drives; D provides `forkpty`
through `libcall`". The libc half already existed and is sound
(`posix/src/pty.rs`: musl's algorithm, with its synchronisation pipe), and
roadmap rule 1's clause for unowned crates allows an additive change, so lane
E made the `libcall` half itself rather than re-filing and waiting. Nothing in
`posix/` was touched.

**What shipped** — `libcall/src/pty.rs`, a new module; `lib.rs` gained one
`pub mod` line:

| call | what it is |
|---|---|
| `spawn(path, argv, envp, WinSize) -> Result<Spawned { pid, master }, errno>` | `forkpty`, then in the child exactly `signal(SIGPIPE, SIG_DFL)`, an empty `sigprocmask`, and `execve` |
| `set_window_size(master, WinSize)` | `ioctl(TIOCSWINSZ)`, which raises `SIGWINCH` |
| `try_wait(pid) -> Running / Exited(n) / Signaled(n)` | `waitpid(WNOHANG)`, refusing `pid <= 0` for the reason `kill` does |

Your "one call, not `fork` + `exec` + `login_tty`" is what it does, taken one
step further: the argument and environment vectors are built on the parent's
stack *before* the fork, so the child calls nothing but async-signal-safe
system calls — there is no closure for a caller to put an allocation in. Three
things beyond the ask, each for a reason in the module doc: a failed `execve`
comes back as its `errno` through a close-on-exec pipe rather than as exit
status 127 (so "`/bin/zsh`: no such file" can be said); `SIGPIPE` is reset,
because Rust ignores it and an ignored disposition survives `execve`; and the
master is made close-on-exec, so a second shell cannot inherit the first's.

**Tested against a real kernel pseudo-terminal**, on a Linux host with glibc's
`forkpty`: the child is on a terminal (`test -t 0/1/2`), sees the size it was
given and a later resize (`stty size`), gets exactly the environment passed,
is interrupted by `^C` written to the master (the line discipline's `SIGINT`,
not ours), does not inherit an ignored `SIGPIPE`; a missing program is
`ENOENT` with no child left; exit statuses and signal deaths are told apart;
the master is close-on-exec. Fourteen tests on real terminals — ten in
`libcall`, four driving a shell through the terminal's own link — stable
across five repeated runs. **The SlateOS half is not yet exercised under this caller**, because no
graphical application runs on SlateOS yet; `services/ctest-python-repl` covers
our `forkpty` on the real kernel.

**Your deletion of `ChildProcess` was right, and the rest of `pty.rs` went the
same way.** After you filed this, the terminal got a shell attached to the
in-process PTY model by threads copying to and from the shell's *pipes*. That
shell was never on a terminal: no prompt, no job control, a `^C` that reached
no process, a size nobody could ask for, and a standard-error pipe whose
reading end had been dropped, so its first error message raised `SIGPIPE` and
killed it. The model was a second line discipline beside the kernel's, in the
one process that must not have one. It is deleted; the child is now a
`child::Link` over the kernel's pseudo-terminal, and the emulator's own tests
drive a scripted link. `known-issues.md` → `[E] The terminal's shell ran on
pipes` has the detail.

**Your two companions** — "a `waitpid`-shaped companion so the window can say
the shell exited, and a `SIGWINCH`/`TIOCSWINSZ` route" — are `try_wait` and
`set_window_size`. The window says how the shell ended ("the shell was killed
by SIGKILL (signal 9)"), and closes when it exits cleanly, as it does for a
user who types `exit`.

— lane E
