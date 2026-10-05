### [E] The terminal's shell ran on pipes, so it was never on a terminal -- 2026-09-24
**Status:** FIXED 2026-09-24 (lane E)

**In short:** On 2026-09-18 `apps/terminal` started a shell, which closed
`TD-C-THE-TERMINAL-ECHOES-AND-RUNS-NOTHING` as FIXED. But the shell ran on
**pipes**, attached by two threads to a model of a terminal that lived inside
the terminal's own process. A shell on pipes does not know it is talking to a
person: it prints no prompt, Ctrl+C cannot stop what it is running, it never
learns the window's size, and full-screen programs refuse to start. Its error
output also went to a pipe nobody read, which kills it the first time it
complains. The shell now runs on a real kernel pseudo-terminal.

**What the 2026-09-18 code did** (`apps/terminal/src/main.rs`, `start_shell`,
deleted here): `Command::new($SHELL or /bin/sh)` with piped stdin, stdout and
stderr; one thread copying stdout into the model's slave end, one copying the
model's slave end into stdin. Measured by reading it, each a property of pipes
rather than a slip in the threads:

| | |
|---|---|
| `isatty(0)` in the shell | false: non-interactive, so no prompt and no job control |
| `^C` | the model turned it into a `PtySignal::Interrupt` value; no process received anything |
| window size | written into the model's `WinSize`, which no process can ask for |
| full-screen programs | `vi`, `less`, `top` need a terminal and refuse without one |
| standard error | `Stdio::piped()`, and the `Child` holding its reading end was dropped when `start_shell` returned -- so the shell's first write to stderr (an error message, such as `command not found`) got `EPIPE`, and with `SIGPIPE` at its default, which `Command` restores in the child, the shell died |
| the shell's exit | never waited for: a zombie, with no status to report |

**Why nobody saw it.** On the Windows development host, where the suite runs,
`/bin/sh` does not exist, so the pipe path never ran; the window said "No
shell" and echoed typing through the model's line discipline. On SlateOS no
graphical application runs yet. Every test drove the model directly. The
defect was reachable only on a Unix host with a compositor, which this project
does not have.

**The fix.** The child is now a `child::Link` (`apps/terminal/src/child.rs`).
The real link is the user's shell on a kernel pseudo-terminal, started by the
new `libcall::pty::spawn` (`forkpty` and `execve` in one call, the vectors
built before the fork) over lane D's existing `posix::pty::forkpty`. A reader
thread drains the master through a bounded channel, so a flood of output is
paced by the kernel's buffer rather than by this process's memory; a writer
thread sends keystrokes, so a child that stops reading cannot freeze the
window. Resizes go out as `TIOCSWINSZ`. The exit is reported once, after the
child's last output, with its status or signal. The model (`pty.rs`, 2,054
lines) is deleted: it was a second line discipline beside the kernel's, in the
one process that must not have one. Rationale: `design-decisions.md` §1200.

**Verified**, against real pseudo-terminals on a Linux host (glibc's
`forkpty`, `/bin/sh`), under `cargo test --target x86_64-unknown-linux-gnu`
in WSL: 10 tests in `libcall` (on a terminal, size and resize, exact
environment, `^C` through the line discipline, `SIGPIPE` restored, a missing
program is `ENOENT` with no child, exit statuses and signal deaths, reaped
once, master close-on-exec) and 4 in `apps/terminal` driving a shell through
the terminal's own link. All stable over five repeated runs. On the host: 80
terminal tests; the mutation table, rewritten for the new child, 70 rows, all 70 caught by the tests named for them -- 23 of them the child's, new or rewritten for the link.

**Not verified: SlateOS.** No graphical application runs there yet.
`todo.txt` → Lane E → "The terminal has never run on SlateOS" lists what to
check the first time one does.

**Found alongside, and fixed in the same change:** `libcall`'s test
`a_single_pid_reaches_the_libc_arm` asserts that `kill(1, SIGTERM)` returns
`ENOSYS`. That is true only on the host build. Built for any Unix target the
call is real: it sends a terminate request to PID 1 -- init -- and the test
fails (`EPERM` as an ordinary user, measured on Linux). Run as root it would
signal init. Nothing ran this crate's tests on a Unix target until this
change's Linux run found it; the test is now `#[cfg(not(unix))]`.
