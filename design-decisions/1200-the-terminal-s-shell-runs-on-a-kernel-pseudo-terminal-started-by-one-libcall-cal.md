## 1200. The terminal's shell runs on a kernel pseudo-terminal, started by one `libcall` call; the terminal's in-process model of a terminal is deleted

**Date:** 2026-09-24
**Lane:** E
**Decided by:** Claude (autonomous)

**In short:** A terminal window has to run a shell, and the shell has to
believe it is talking to a terminal — that belief is what makes it print a
prompt, lets Ctrl+C stop a command, and tells a full-screen program such as an
editor how big the window is. Our terminal application had been *simulating* a
terminal inside its own process and running the shell on plain pipes beside
the simulation, so the shell never believed it: no prompt, no Ctrl+C, no size.
This replaces the simulation with the real thing the kernel already provides,
and records the choices that came with it — how the shell is started, how the
terminal's own tests work without a shell, and what the window does when the
shell ends.

### What forced it

`apps/terminal/src/pty.rs` (2,054 lines) modelled a pseudo-terminal —
master, slave, a cooked-mode line discipline (the part that buffers a line and
echoes it), a window size — as Rust objects inside the terminal. On
2026-09-18 a shell was attached to the model's slave by two threads copying to
and from the shell's *pipes*. Measured consequences, each one a property of
pipes rather than a bug in the threads:

| | |
|---|---|
| `isatty(0)` in the shell | false, so it ran non-interactively: no prompt, no job control |
| `^C` | became a `PtySignal::Interrupt` value that no process received |
| window size | stored in a struct the shell could not ask for, so it never learned it |
| full-screen programs | refuse to start without a terminal |
| standard error | a pipe whose reading end was dropped when `start_shell` returned, so the shell's first error message raised `SIGPIPE` and killed it |
| the exited shell | never reaped, and its exit status never read |

The kernel has had real pseudo-terminals since 2026-08-23 (syscalls 544–556,
`kernel/src/tty/pty.rs`), and `posix::pty::forkpty` composes them the way musl
does. Design-decisions §768 says a program reaches stateful libc through the
C ABI — `libcall` — so the missing piece was a `libcall` door and the terminal
using it.

### The choices, and what else was on the table

**1. The kernel's pseudo-terminal, and no model beside it.**

- *Chosen — the model is deleted.* *What changes:* the shell is interactive;
  `^C`, `SIGWINCH` and full-screen programs are the kernel's business.
- *Keep the model as a fallback for hosts without pseudo-terminals.* *What
  changes:* on the Windows development host the window would echo typing
  locally. Rejected: it is a second line discipline in the one process that
  must not have one, 2,000 lines to keep in step with the kernel's, and its
  echo is exactly what made a terminal with no shell look like a terminal
  with a quiet one — the defect `known-issues.md`
  `TD-C-THE-TERMINAL-ECHOES-AND-RUNS-NOTHING` was filed about.
- *Pipes* (the 2026-09-18 design). Rejected by the table above.

**2. One call that forks and execs, with everything built before the fork.**

`libcall::pty::spawn(path, argv, envp, size)`: the argument and environment
vectors are laid out on the parent's stack, then `forkpty`, then the child does
nothing but restore `SIGPIPE`, empty its signal mask and `execve`.

- *A `fork` + `exec` pair, or a hook run in the child* (`std`'s `pre_exec`
  shape). *What changes:* a caller could run arbitrary code between the two.
  Rejected: in a multi-threaded process the child of a fork owns copies of
  locks other threads held — the allocator's included — so the only safe code
  there is async-signal-safe system calls, and an API that accepts a closure
  cannot enforce that. The request that asked for this call said the same
  (`requests/c-b-a-terminal-needs-a-shell-on-the-other-end-of-its-pty.md`).
- *Cost of the choice:* `libcall` is `no_std` with no allocator, so the
  vectors are fixed arrays — at most 256 arguments and 1,024 environment
  entries, `E2BIG` beyond. Far above anything a terminal passes; stated so the
  bound is not a surprise to the next caller.
- Two additions beyond the ask, each fixing a real fault: a failed `execve`
  is reported as its `errno` through a close-on-exec pipe, not guessed from
  exit status 127 (so the window can say "`/bin/zsh`: no such file"); and the
  master is made close-on-exec, so a second shell cannot inherit the first's
  master and keep it from ever being hung up.

**3. A seam, `child::Link`, and a scripted child for the emulator's tests.**

- *Chosen.* The emulator's tests assert what it *sends* for a key and what it
  *draws* for a byte against a script; the real link has its own tests that
  drive a real shell over a real pseudo-terminal.
- *Test the emulator against a real shell.* Rejected: every emulator test
  would depend on a platform, a shell and a clock, and on the Windows host
  where the suite runs there is no pseudo-terminal at all.
- *Cost:* the real link is `cfg(unix)`, so the host test run and the mutation
  sweep cannot compile it. Its tests run on a Linux host (`cargo test -p
  terminal --target x86_64-unknown-linux-gnu`, under WSL here) — a second
  command someone has to know to run, recorded in `apps/terminal/mutate.py`
  and `known-issues.md`.

**4. A clean exit closes the window; any other ending leaves it open.** A
user-visible policy decided without asking, so it is also in `todo.txt` →
Lane E → Judgment Calls, where the operator can overrule it. The
alternatives — always close (xterm and most Linux desktops; loses the crash
message) and never close (every `exit` needs a second close) — are one branch
away.

**5. The terminal polls for output until the window library can wake it.**
`oswindow` wakes an application only for compositor events and a clock, so
output is read on a tick: 16 ms while the shell is talking, 50 ms once it has
been quiet for two seconds. That is an interim with a named end —
`requests/e-f-wake-an-application-for-its-own-descriptor.md` — not a design.

### What is verified, and what is not

Verified on a Linux host against glibc's `forkpty`: the child is on a terminal,
sees its size and a later resize, gets exactly the environment passed, is
interrupted by `^C` written to the master, does not inherit an ignored
`SIGPIPE`; a missing program is an error with no child left; exits and signal
deaths are told apart; the master is close-on-exec; a command typed through the
terminal's own link runs and answers, hang-up ends the shell, and the last
output arrives before the exit. **Not verified: any of it on SlateOS**, because
no graphical application runs there yet. `todo.txt` → Lane E lists what to
check the first time one does.

**Where it lives:** `libcall/src/pty.rs`; `apps/terminal/src/child.rs`;
`TerminalState::{attach, drain_child, flush_to_child, tick_interval}` in
`apps/terminal/src/main.rs`.

**How to reverse:** the model is `apps/terminal/src/pty.rs` in the commit
before this one. Nothing else in the tree used it.
