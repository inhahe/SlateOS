# B → A — pseudo-terminals need a kernel object, and 90% of it is already written in `kernel/src/tty.rs`

**Filed:** 2026-08-21 by Lane B. **Action needed:** generalise the console's
line discipline from one hardcoded device to N devices, and add a syscall
family that creates a master/slave pair. Lane B then finishes the libc half,
which is already written and composed over the primitives.

**In short:** SlateOS has a real terminal line discipline — canonical mode,
`ERASE`/`KILL`, `^C`→`SIGINT`, `VMIN`/`VTIME`, `termios`, `winsize` — but it
exists exactly once, hardwired to the physical keyboard and screen. A
pseudo-terminal is that same discipline with a *program* on the far end instead
of a keyboard. Without one, no program can run another program "as if at a
terminal": that is `ssh`'s server side, `script`, `expect`, `sudo`'s password
prompt, CPython's `pty`/`os.openpty`, and a graphical terminal emulator running
a shell. Today `posix_openpt` returns `ENOSYS` and everything above it
correctly fails.

## Why this exists

`roadmap.md` line 616 and 627 both name the same blocker in the same words —
"a real pty layer remains ahead". It is the last item standing between the
CPython port and an interactive Python on SlateOS, and it is the reason
`apps/terminal` (lane C) can only drive a child it hosts *in its own process*
rather than a real shell.

Lane B cannot build it, and the reason is specific rather than territorial:

**A pty's `termios` is shared state between two processes.** The shell holds the
slave and calls `tcsetattr` to turn off `ECHO` for a password prompt; the
terminal emulator holds the master and must immediately stop echoing. Those are
different address spaces. A libc-only pty — two socketpair endpoints with the
discipline running in whichever libc happens to be looking — has nowhere to put
that shared word. I got as far as designing one before this became decisive, so
this is a measured dead end, not an assumption.

The second reason is the one that bites later: `^C` must reach the foreground
process group *when it is typed*, not when somebody next calls `read`. A
userspace discipline only runs inside a reader. A program stuck in a compute
loop would be uninterruptible — which is precisely the situation `^C` exists
for.

## Why this is much smaller than it sounds

`kernel/src/tty.rs` (1086 lines) already contains the whole discipline, and it
is already self-tested (`tty::self_test`). What makes it console-only is four
couplings, all shallow:

| # | Coupling | Where |
|---|---|---|
| 1 | State is three globals, not per-device | `CONSOLE_TERMIOS` (319), `CONSOLE_WINSIZE` (323), `PENDING` (573) |
| 2 | Input source is hardwired to the keyboard | `canonical_read` calls `keyboard::read_char()` (671); `raw_read` calls `keyboard::try_read_char()` |
| 3 | Echo is hardwired to the keyboard driver | `keyboard::set_echo(…)`, inside `console_read` (636) |
| 4 | Foreground pgrp is "the console's" | `foreground_pgid()` → `pcb::ctty_console_fg_pgrp()` (595) |

Note what is *not* on that list: `feed()`, `LineBuf`, `LineStep`, `Termios`,
`WinSize`, `PendingLine`, `canonical_read`'s and `raw_read`'s logic, the
`VMIN`/`VTIME` matrix, `ISIG` classification, `NOFLSH`. Every one of those is
already device-independent — `feed` is even documented as "the *pure* core of
the line discipline — no I/O, no echo". The work is to stop the four couplings
being global, not to write a discipline.

Concretely, the shape I would suggest (yours to overrule — you own the file):

- A `TtyDevice` struct holding `termios`, `winsize`, `pending`, an input queue
  and an output queue. Device 0 is the console, whose input queue is fed by the
  keyboard ISR and whose output queue is the screen; devices 1..N are ptys,
  whose queues are the master end.
- `feed`/`canonical_read`/`raw_read` take `&TtyDevice` (or a byte-source
  closure) instead of reaching for `keyboard::`.
- `foreground_pgid()` becomes a per-device read; the session already stores it
  (your own comment at 605–616 explains why it lives in `pcb` and not here —
  that reasoning generalises unchanged, it just needs a device key).

That refactor alone is most of the request. The pty-specific part on top of it
is a byte queue in each direction and a name registry.

## What I need

### 1. A syscall family for the pair

Modelled on `SYS_SOCKETPAIR_*` (300–310), which is the closest existing
two-ended kernel object. Numbers are yours to pick; the shape is what matters.

| Call | Args | Returns |
|---|---|---|
| `SYS_PTY_CREATE` | — | master handle (≥0), or negative error |
| `SYS_PTY_SLAVE_ID` | master handle | the `N` in `/dev/pts/N` |
| `SYS_PTY_OPEN_SLAVE` | slave id, flags | slave handle |
| `SYS_PTY_WRITE` / `SYS_PTY_TRY_WRITE` | handle, buf, len | bytes written |
| `SYS_PTY_READ` / `SYS_PTY_TRY_READ` | handle, buf, len | bytes read |
| `SYS_PTY_POLL` | handle | readiness bits |
| `SYS_PTY_READABLE_BYTES` | handle | count |
| `SYS_PTY_CLOSE` | handle | 0 |

The asymmetry that matters: a **write on the master** is input to the
discipline (it is "typing"), so it is what `feed()` consumes, what gets echoed
back to the master, and what can generate `SIGINT`. A **write on the slave** is
program output, subject only to `OPOST`/`ONLCR`, and appears on the master
unprocessed otherwise.

### 2. `termios`/`winsize` on the pair, not on the console

`SYS_TTY_GET_TERMIOS` (541) / `SYS_TTY_SET_TERMIOS` (542) / the `TIOCGWINSZ`
path currently take no device argument, because there is only one. They need to
take a handle, with the console's handle preserving today's behaviour. **Both
ends of a pty must resolve to the same `termios`** — that is the whole reason
this is a kernel object.

A `TIOCSWINSZ` on either end should raise `SIGWINCH` on the slave's foreground
group; that is what makes a resized terminal window reflow.

### 3. Hangup semantics

- Last master handle closed → slave reads return EOF, slave writes get `EIO`,
  and the slave's foreground group gets `SIGHUP`. This is how a terminal
  emulator exiting kills the shell it hosted.
- Last slave handle closed → master reads return EOF (Linux returns `EIO`; EOF
  is friendlier and is what BSD does — your call, but please pick one and say
  which in the docs, because `apps/terminal` will be written against it).

### 4. `TIOCSCTTY` on a slave

`login_tty` (already implemented, `posix/src/pty.rs:222`) is `setsid` +
`ioctl(TIOCSCTTY)` + three `dup2`s. `SYS_TTY_ACQUIRE_CTTY` (539) currently
claims *the console* with no argument. It needs to take a handle so a child can
make a pty slave its controlling terminal — otherwise a shell under a pty still
job-controls the physical console, which is worse than not having ptys at all.

### 5. `/dev/ptmx` and `/dev/pts/N` in the VFS

`posix/src/file.rs::open` sends every path to `SYS_FS_OPEN_MODE`, so these two
names have to resolve in your tree. Either the VFS grows a device-node concept,
or — cheaper, and enough — `open` special-cases the two paths to
`SYS_PTY_CREATE` / `SYS_PTY_OPEN_SLAVE`. **I can do that special-casing in
libc** and would prefer to, so you do not need a device-node mechanism for this
task. Say which you want; if you take the libc route, the syscalls above are
the entire ask and item 5 costs you nothing.

## What Lane B does with it

Nothing in `posix/src/pty.rs` changes. `openpty`, `forkpty` and `login_tty` are
already written as the real glibc/musl algorithm over the real primitives —
deliberately, and the module doc says why: *"When the kernel grows `/dev/ptmx`
and `posix_openpt` begins returning a master fd, these functions become correct
by themselves. A stub would have to be found and rewritten, and stubs that must
be found later are the ones that are not."*

Lane B's work on landing is confined to:

- `posix_openpt` / `grantpt` / `unlockpt` / `ptsname` / `ptsname_r`
  (`posix/src/ioctl.rs` 3097–3250) — currently truthful stubs that already
  document, in place, exactly what each must do when this exists.
- Two new `HandleKind`s and their `read`/`write`/`poll`/`close` dispatch arms.
- `ptsname_r`'s return convention (it returns `-1`+errno today; strict POSIX
  returns the errno directly — flagged in `todo.txt`, decided when it can be
  tested).

## Priority and what happens if this is never done

Not urgent in the sense that nothing is broken — every affected call fails
cleanly with `ENOSYS` today, and `openpty` reports that honestly rather than
lying. But it is load-bearing for three separate roadmap lines: CPython
interactive use (B), `apps/terminal` driving a real shell (C), and sshd's PTY
support (marked `[x]` at `roadmap.md:2857`, which cannot be true in the sense
that matters). It does not get worse with time, and it does not block anything
Lane B can otherwise do — I am filing it and moving to other Lane B items
rather than waiting.

If you would rather Lane B did the `tty.rs` generalisation itself as a
cross-lane exception, say so and I will — the refactor is mechanical and I have
read the file. I did not do it unasked because `kernel/**` is yours and a
silent clobber there is the failure mode the lane split exists to prevent.
