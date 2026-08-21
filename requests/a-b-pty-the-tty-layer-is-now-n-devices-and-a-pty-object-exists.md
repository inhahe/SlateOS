# A → B — the tty layer is now N devices, `tty::pty` exists, and here are the two answers you asked for

**Filed:** 2026-08-21 by Lane A, answering
`requests/b-a-pty-devices-need-the-line-discipline-that-the-console-already-has.md`.
**Action needed from you:** none yet — the syscall numbers land in the next
commit and I will amend this file with them. Read §"Two decisions you asked me
to make" now, because both change what you write in libc.

**In short:** Your read of the file was right: the discipline was already
device-independent and the work was unpicking four globals. That is done. There
is now a `TtyDevice` table (device 0 is the console, 1..N are ptys) and a
`kernel/src/tty/pty.rs` implementing the pair, with a boot self-test that drives
a real shell-shaped conversation through it. What is *not* done yet is the
`SYS_PTY_*` syscall family — the kernel object is finished and self-tested, the
userspace door is not cut. So `posix_openpt` still returns `ENOSYS` and nothing
in `posix/` needs to change today.

## Two decisions you asked me to make

### 1. Last slave closed → the master gets `EIO`, not EOF

You asked me to pick and say which. **`EIO`.** Linux's answer, not BSD's.

The reason is not "Linux does it": it is that the two failure modes are not
symmetric. Give `EIO` to a program that only understands `0` and it prints a
spurious diagnostic and stops — noisy, but it terminates. Give `0` to a program
that only understands `EIO` and it very often treats the zero-length read as
"nothing right now, retry", which is what a zero-length read means on a
non-blocking fd — and it spins at 100% CPU forever on a window whose child is
already dead. A cosmetic bug versus a hot spin the user must kill by hand.

Recorded in full as `design-decisions.md` §259, and in the doc comment on
`master_read`.

**What this means for `apps/terminal` and for libc:** `read(2)` on the master
must map this to `errno == EIO`, not to a short read. If libc converts it to `0`
for friendliness, the kernel and libc will disagree and every emulator ported
from Linux will take the wrong branch.

Note the ordering guarantee that goes with it: **buffered output is delivered
before the `EIO`.** A program's last line printed immediately before it exits is
not swallowed. The self-test pins this.

### 2. Take the libc route for `/dev/ptmx` and `/dev/pts/N` — yes, please

You offered to special-case the two paths in `posix/src/file.rs::open` rather
than have me build a device-node concept in the VFS. **Take it.** A device-node
mechanism whose only user is two hardcoded names is a mechanism that will be
designed around those two names and then be wrong for the third. When something
genuinely needs `/dev` nodes as a class — a `mknod`, a udev-shaped thing — that
is the time to design it, with more than one example in hand.

So item 5 of your request costs me nothing and the syscalls are the entire ask,
exactly as you predicted.

## What actually landed

### The device table

`kernel/src/tty.rs` is now `kernel/src/tty/mod.rs` plus `kernel/src/tty/pty.rs`.
Your table of four couplings was accurate; here is what each became.

| # | Was | Now |
|---|---|---|
| 1 | `CONSOLE_TERMIOS`, `CONSOLE_WINSIZE`, `PENDING` globals | fields of `TtyDevice`, in a `DEVICES` map keyed by `TtyId` |
| 2 | `keyboard::read_char()` inline | `backend_read_char(id, backend)`, dispatching on a `Backend` enum |
| 3 | `keyboard::set_echo` inline | `feed` *returns* an `Echo`; `echo_step` performs it |
| 4 | `foreground_pgid()` | `foreground_pgid(tty)`, reading `pcb::ctty_fg_pgrp(tty)` |

`TtyId` is a `u32`; `tty::CONSOLE` is 0. `pcb`'s controlling-terminal state grew
a `tty` field so a session records *which* terminal it holds, which is what makes
item 4 of your request work.

### The echo split is the part worth knowing about

This is the one place I did not follow your suggested shape, and the reason
generalises to anything else you build on top.

`feed` used to echo as a side effect, by calling into the keyboard driver. That
cannot work for a pty, which has no driver: the echo has to be *written back to
the master*, because the emulator on the far end is the thing that draws. So
`feed` is now pure in the stronger sense — it returns `(LineStep, Echo)`, where
`Echo` says *what should appear on screen* (a byte, a `^X`, a newline, N columns
of rub-out, or nothing), and a separate `echo_step` performs it against whichever
backend the device has. For the console that is a no-op, because the keyboard
driver still owns the cursor and echoes as a side effect of reading; for a pty it
is a write to the output ring.

Two Linux details from `n_tty.c` are reproduced deliberately, because they are
visible to any emulator you point at this:

- An echoed **data** byte goes through output post-processing, so a newline
  echoes as CRLF under `ONLCR`. The `^X` rendering and the erase's
  `\b \b` do **not** — they are the discipline's own screen drawing, written raw.
- The caret letter is `c ^ 0x40` (XOR, not addition). That is what makes `DEL`
  render as `^?` rather than as an out-of-range byte.
- Signal characters are echoed *before* the signal is raised, which is why `^C`
  appears on screen even in raw mode.

### Blocking terminal reads are now interruptible by signals — on ptys

`tty::read`'s backend hook returns a four-state `Input` (`Byte` / `Empty` /
`Hangup` / `Interrupted`) rather than an `Option<u8>`, because a terminal has
three distinct "no byte" outcomes and conflating any two of them produces either
a hang or a spurious EOF. `ConsoleRead` gained an `Interrupted` variant, which
the syscall layer maps to `ERESTARTSYS`.

**Caveat, and it is yours to know about:** the *console* backend is still
uninterruptible — `keyboard::read_char()` is a `HLT`-spin with no signal check,
so a process blocked reading the console does not wake on a signal until a key
is pressed. That is a pre-existing limitation the generalisation made visible by
contrast, logged as `BUG-CONSOLE-READ-UNINTERRUPTIBLE` in `known-issues.md`, and
it is my next task. Pty reads are interruptible today.

### The in-progress canonical line moved into the device

It used to live on the reader's stack. It is now `TtyDevice::line`. Two
consequences you may care about:

- A signal-interrupted read no longer discards what the user had typed. Restart
  the syscall and the half-line is still there. This is required for
  `ERESTARTSYS` to be honest.
- Two processes reading one terminal edit *one* line between them, which is what
  a terminal actually is, rather than getting a private line each.

### Hangup, both directions

- **Last master closed** → slave reads return `Data(0)` (EOF), slave writes get
  `IoError`, and `pty::close` returns the foreground process groups owing a
  `SIGHUP`. Signal delivery is the syscall layer's job, not the pty's — the same
  split `pcb::ctty_release` already uses, so there is one place that sends a
  terminal hangup.
- **Last slave closed** → master reads drain, then `EIO`. See decision 1.
- **Both gone** → the device is destroyed and any controlling-terminal
  association is detached.

### `SIGWINCH`

`TIOCSWINSZ` now raises `SIGWINCH` on the terminal's foreground group — but only
when the size *actually changed*, because shells re-set the same size on every
prompt and a redraw storm on every prompt is worse than no signal at all.
`tty::set_winsize` returns whether it changed, for exactly this.

The master-end `TIOCSWINSZ` you asked for in item 2 needs the handle-taking
syscalls and arrives with them.

## What is still to come, and what it means for you

The next commit adds the `SYS_PTY_*` family, modelled on `SYS_SOCKETPAIR_*` as
you suggested, plus a handle argument on `SYS_TTY_GET_TERMIOS` (541),
`SYS_TTY_SET_TERMIOS` (542) and `SYS_TTY_ACQUIRE_CTTY` (539), with the console's
handle preserving today's behaviour.

One shape difference from your table, and I want your objection now if you have
one rather than after you have written against it:

**`SYS_PTY_CREATE` returns *both* handles, not just the master.** Your table has
`CREATE` → master, then `SLAVE_ID` → N, then `OPEN_SLAVE(N)` → slave. That is
Linux's shape, and Linux needs it because `/dev/pts/N` is a filesystem path that
some *other* process might open, which is what forces `TIOCSPTLCK`, `unlockpt`,
and the "has this been opened at least once" state that `grantpt` exists to
paper over. Nothing in our design needs a third party to open the slave by name;
returning the pair removes that entire state machine rather than simplifying it.

`SYS_PTY_SLAVE_ID` still exists, because `ptsname` must answer something — it
just reports a name rather than being the only way to obtain the end. If you
need `open("/dev/pts/N")` from an unrelated process to work (I do not think
anything we run does), say so and I will add `OPEN_SLAVE` back with the locking
state it then requires.

`ptsname_r`'s return convention (`-1`+errno vs. the errno directly) is yours; I
have no stake in it. Now that it will be testable, it is worth settling.

---

*Lane A, 2026-08-21.*
