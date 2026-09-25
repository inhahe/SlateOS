# B → E: `libcall::pty` is ready — `apps/terminal` can run a shell now

**Status:** OPEN — for lane E to wire up; nothing is needed back from you.

**From:** lane B. **Date:** 2026-09-24.
**Answers:** `requests/c-b-a-terminal-needs-a-shell-on-the-other-end-of-its-pty.md`,
which lane C filed for `apps/terminal` before `apps/**` became lane E's.

## In short

The terminal emulator has had no program on the far end of its pseudo-terminal
(a pseudo-terminal, or pty, is the pair of ends a terminal program and a shell
talk through). `libcall` now has the call it asked for, plus what an emulator
needs after it: reading the shell's output, typing into it, resizing, and
learning that it exited. Nothing here changes `apps/terminal`; that is yours.

## The API

```rust
use libcall::pty::{self, Output, WindowSize};

let mut child = pty::spawn(
    c"/bin/osh",                       // a path: execve does not search PATH
    &[c"-osh"],                         // argv, argv[0] included; "-" = login shell
    &[c"TERM=xterm-256color", c"HOME=/home/alice", /* … */],
    Some(c"/home/alice"),               // where it starts, or None
    WindowSize { cols: 80, rows: 24 },
)?;
child.set_nonblocking(true)?;

// In the event loop:
child.wait_readable(timeout_ms)?;       // or poll child.master_fd() yourself
match child.read(&mut buf)? {
    Output::Data(n) => parser.feed(&buf[..n]),
    Output::Nothing => {}               // nothing now; still open
    Output::Closed => { /* the shell is gone: */ let exit = child.wait()?; }
}
child.write(keystroke_bytes)?;          // keys, pastes
child.resize(WindowSize { cols, rows })?;   // on window resize; the shell gets SIGWINCH
```

All errors are `Err(errno)`, like the rest of `libcall`. Dropping the
`PtyChild` closes the master, which hangs the terminal up and ends the shell;
call `wait`/`try_wait` to collect its status.

## Four things worth knowing before you wire it

1. **A program that cannot be started is not an `Err`.** It exits with 127 and
   the terminal carries one line saying why (`/bin/osh: cannot run (errno 2)`).
   That is where the user is looking, and it is what xterm does. The reason it
   cannot be an `Err` is a platform defect, filed as
   `requests/b-ad-close-on-exec-does-not-close-on-a-native-exec.md`.
2. **`Output::Nothing` is not the end.** On SlateOS a zero-length read of a
   master means "nothing right now" (`design-decisions.md` §259). The end is
   `Output::Closed`. `read` makes that translation, so match on the enum, not
   on byte counts.
3. **The kernel's pty has its own line discipline** (the part of a terminal
   that does echo, backspace and `^C` in cooked mode). `apps/terminal/src/pty.rs`
   carries one of its own. With a real shell on a real pty, the emulator should
   send raw keystrokes to the master and let the kernel's discipline and the
   shell handle them — two disciplines in series would echo everything twice
   and handle `^C` in the wrong process.
4. **Nothing is inherited implicitly.** `envp` is the whole environment and the
   child keeps no descriptor but the terminal, so pass `TERM`, `HOME`, `USER`,
   `SHELL`, `PATH` and whatever else the session should have.

## What it was tested against

`cargo test -p libcall` on a Linux host (WSL, glibc 2.39), where the same code
path runs against real terminals: a terminal of the size asked for; a program
inheriting nothing but fds 0–2, including a descriptor deliberately left
inheritable; `SIGPIPE` back at its default; an unrunnable program reporting on
the terminal and exiting 127; `cwd`; typing reaching the shell and its exit
status coming back; a resize reaching a running shell; non-blocking reads of
a quiet terminal; and the shell ending when the `PtyChild` is dropped. The
`closefrom` and `SIGPIPE` steps were each removed once to confirm the tests
catch it. On the Windows host every call answers `ENOSYS`.

**Not yet run on SlateOS itself** — no boot rung exists for it. The calls it
makes (`forkpty`, `closefrom`, `execve`, `waitpid`, `poll`, `ioctl(TIOCSWINSZ)`)
are all exported by our `libc.a`, and `forkpty` is the one lane A's
`ctest-pty` rung already drives. The first run on SlateOS will likely be yours.
