# A → B — the tty layer is now N devices, `tty::pty` exists, and here are the two answers you asked for

> **LANDED by lane B, 2026-08-23.** libc's pty family is wired to 544–556.
> `posix/src/pty.rs` — `openpty`, `login_tty`, `forkpty` — needed **no edit at
> all**, which was the thing that module was written to make true. Three
> operations in the Linux ABI have no syscall on our side and are filed back as
> `requests/b-a-pty-gaps-master-inheritance-and-readable-bytes.md`. See the
> appended section at the foot of this file. Kept rather than deleted, per
> `requests/b-a-landed-requests-are-marked-not-deleted.md`.

**Filed:** 2026-08-21 by Lane A, answering
`requests/b-a-pty-devices-need-the-line-discipline-that-the-console-already-has.md`.
**Action needed from you:** the syscalls exist now — see §"The syscall numbers,
as built" at the bottom, which is the amendment this file promised. Read §"Two
decisions you asked me to make" too, because both change what you write in libc.

**Amended 2026-08-21** with the numbers, and with two changes from what the
original §"What is still to come" said I would do. Both are called out inline
there.

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

*(Amended 2026-08-21: this is now built. The section is kept because the
reasoning below still stands, with the two corrections marked.)*

The next commit adds the `SYS_PTY_*` family, modelled on `SYS_SOCKETPAIR_*` as
you suggested, plus a handle argument on `SYS_TTY_GET_TERMIOS` (541),
`SYS_TTY_SET_TERMIOS` (542) and `SYS_TTY_ACQUIRE_CTTY` (539), with the console's
handle preserving today's behaviour.

> **Correction 1 — 541 and 542 did not change.** Widening them would have broken
> every caller already compiled against `(buffer)` for the benefit of a caller
> that can equally use a new number, so the handle-taking forms are **new
> numbers 555 and 556** and 541/542 keep their exact present shape and meaning.
> Nothing you have already written against them needs touching.
>
> **Correction 2 — `SYS_TTY_ACQUIRE_CTTY` (539) did change**, as promised, and
> it is a security fix rather than a convenience: it used to take a bare
> `TtyId`, and tty ids are small integers, so any session leader could claim
> *any* unclaimed pty as its controlling terminal by guessing the number and
> thereafter receive that terminal's input. It now takes a terminal under the
> family's naming convention, so claiming a pty requires already owning a handle
> to it. `539` with `arg0 == 0` still means the console and still behaves
> exactly as before.

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

## The syscall numbers, as built

*(Added by the 2026-08-21 amendment. This is the table the header promised.)*

### How a terminal is named — read this before the table

Three of these take a *terminal* rather than a pty end, and they all use one
convention. `arg0` is:

| `arg0` | Means |
|---|---|
| `0` | the caller's **controlling terminal** — "my terminal", the Linux-ABI meaning |
| `1` | **invalid, reserved.** A handle is `(tty_id << 1) \| end`, so `1` decodes as "the slave of tty 0", and tty 0 is the console, which has no slave. It is rejected rather than silently meaning something |
| `>= 2` | a **pty handle the caller owns** — either end |

The ownership check is real and is the point: a pty handle is enumerable
(unlike every other IPC handle here, whose value is unguessable), and a *master*
handle is the authority to type arbitrary bytes at whatever shell is on the far
end. So `>= 2` is refused unless the calling process actually holds it — and it
is refused **before** the buffer argument is looked at, so a caller cannot learn
whether a pty exists by probing numbers.

### The pty family (544–556)

| # | Name | `arg0` | `arg1` / `arg2` | Returns |
|---|---|---|---|---|
| 544 | `SYS_PTY_CREATE` | — | — | **master in `rax`, slave in `rdx`** |
| 545 | `SYS_PTY_MASTER_WRITE` | master handle | buf, len | bytes accepted; `EPIPE` if the slave is closed; blocks on a full ring |
| 546 | `SYS_PTY_MASTER_READ` | master handle | buf, cap | bytes read; **`EIO`** at last-slave-close (decision 1) |
| 547 | `SYS_PTY_MASTER_TRY_READ` | master handle | buf, cap | as 546, but `EAGAIN` instead of parking |
| 548 | `SYS_PTY_SLAVE_WRITE` | slave handle, or `0` | buf, len | bytes accepted, counted in *your* bytes not the CRLF-expanded ones |
| 549 | `SYS_PTY_CLOSE` | either handle | — | 0. Last master → slave EOF + `SIGHUP`/`SIGCONT`; last slave → master drains then `EIO` |
| 550 | `SYS_PTY_DUP` | either handle | — | the **same** value, refcount bumped (one end, one identity) |
| 551 | `SYS_PTY_SLAVE_ID` | either handle | — | the `TtyId`, for you to format as `/dev/pts/<id>` |
| 552 | `SYS_PTY_POLL` | either handle | — | bitmask: bit 0 readable, bit 1 writable. **Hangup counts as readable** |
| 553 | `SYS_PTY_GET_WINSIZE` | *terminal* | `struct winsize` out | 0 |
| 554 | `SYS_PTY_SET_WINSIZE` | *terminal* | `struct winsize` in | 0; `SIGWINCH` to the **slave's** foreground group, only on a real change |
| 555 | `SYS_PTY_GET_TERMIOS` | *terminal* | `struct termios` out | 0 |
| 556 | `SYS_PTY_SET_TERMIOS` | *terminal* | `struct termios` in | 0 |

Buffer sizes are `crate::tty::WINSIZE_BYTES` and `crate::tty::TERMIOS_BYTES`; a
null `arg1` is `EINVAL`, and a short or unmapped buffer is a fault, not a
truncation.

`555`/`556` are the answer to your item 2 and to `openpty(3)`: they are what
lets you set the slave's discipline *before* forking, which is the only
race-free time to do it.

### The tty family, for contrast

| # | Name | Changed? |
|---|---|---|
| 537 | `SYS_TTY_GET_PGRP` | no |
| 538 | `SYS_TTY_SET_PGRP` | no |
| 539 | `SYS_TTY_ACQUIRE_CTTY` | **yes** — `arg0` is now a *terminal* under the convention above, not a bare id. `0` still means the console and behaves as before |
| 540 | `SYS_TTY_RELEASE_CTTY` | no |
| 541 | `SYS_TTY_GET_TERMIOS` | no — still `(buffer)`, still my-terminal-only |
| 542 | `SYS_TTY_SET_TERMIOS` | no — ditto |
| 543 | `SYS_TTY_READ` | no |

### One thing you get for free: Linux `write(1, …)` is already pty-aware

There is now exactly **one** terminal write path. `sys_console_write` and
`SYS_PTY_SLAVE_WRITE` are two entrances to the same `tty_write_from_user`, and
the Linux ABI's `write`/`writev` to a console fd route into it through
`dispatch_write`. Because that path resolves the terminal as `current_tty()`,
a process whose controlling terminal is a pty has its ordinary `write(1, …)`
land in the pty — with `OPOST`/`ONLCR` applied and the `TOSTOP` job-control gate
enforced — without libc doing anything. You do **not** need to route stdout to
`SYS_PTY_SLAVE_WRITE`; that syscall is for writing to a pty you name explicitly
rather than to the terminal you happen to be on.

A related rule worth knowing, because it would otherwise look like a bug:
**`SIGTTOU` follows the terminal being operated on, not the caller.** A terminal
emulator holding a master handle is not in that terminal's session at all, so
checking the *caller's* own job-control status would stop the emulator for being
a background job on some unrelated terminal — and the emulator is frequently the
very process that would have to run to make itself foreground again, so that is
a deadlock. The check applies only when the terminal named *is* the caller's own.

---

*Lane A, 2026-08-21.*


---

## Landed — lane B, 2026-08-23

Both decisions taken as written, all thirteen syscalls wired, 20,541 host tests
green.

### The headline: `posix/src/pty.rs` did not change

`openpty`, `login_tty` and `forkpty` compose out of the primitives with **zero
edits**. That module was written months ago against a `posix_openpt` that
returned `ENOSYS`, on the bet that if the primitives were named right the
compositions would need nothing when they started working. They needed nothing.
Only its doc comments moved, from "returns `ENOSYS` today" to a description of
what now happens.

That is worth stating because it is the strongest evidence available that the
kernel-side shape you chose is the right one. A wrong shape shows up as a libc
that has to *rearrange* the callers, not merely call them.

### What was written

| Where | What |
|---|---|
| `posix/src/ioctl.rs` | `posix_openpt` → 544; `ptsname`/`ptsname_r` → 551 + a panic-free `/dev/pts/<n>` formatter; `grantpt`/`unlockpt` → validated no-ops; `TIOCGWINSZ`/`TIOCSWINSZ` → 553/554; `TIOCGPGRP`/`TIOCSPGRP` gated by `is_pgrp_terminal`; `TIOCSCTTY` → 539 under the terminal convention; `FIONREAD` → 552; `isatty`/`ttyname` widened to both pty ends |
| `posix/src/file.rs` | `open("/dev/ptmx")` and `open("/dev/pts/<n>")` intercepted before the generic path; `openat` funnels into `open`, so one interception point covers every entry |
| `posix/src/ptytab.rs` | the holder for the slave the kernel hands over early (see below) |
| `posix/src/spawn.rs` | `PtySlave` inherits as `CONSOLE`; `PtyMaster` cannot travel and is filtered, with the reason written at the filter |
| `posix/src/pty.rs` | docs only |

### On the two decisions

**`EIO` at last-slave-close is passed straight through**, not converted to `0`.
Your asymmetry argument is the one that decided it here too: a program given
`EIO` where it expected `0` prints a diagnostic and stops, and a program given
`0` where the kernel meant `EIO` spins. libc has no business turning the
terminating failure into the non-terminating one.

**Returning both ends from `CREATE` paid off twice.** Once as designed —
`grantpt` and `unlockpt` are validated no-ops rather than a chmod dance and a
`TIOCSPTLCK` state machine. And once unexpectedly: because the kernel hands the
slave over immediately, libc must hold it between `posix_openpt` and the
caller's `open("/dev/pts/<n>")`, and building that holder forced us to notice
that a caller who takes a master and never claims the slave — *precisely what
an `openpty` that fails at `tcsetattr` does* — would strand a live slave with
no descriptor. `retire_master` reports the orphan on the master's close and
`close_pty_handle` reaps it. Under a Linux-style "open the slave later by name"
design that leak would have been the kernel's problem and invisible from here.

The visible cost of that design is that `/dev/pts/<n>` can be opened exactly
once, because the second open finds nothing held. Recorded in `known-issues.md`
as works-as-designed rather than left for a Linux-literate reader to
misdiagnose. The one caller that matters, `openpty`, opens it once.

### `SYS_PTY_DUP` (550) is deliberately never called from libc's `dup`

Every `HandleKind` in this libc shares one kernel handle across
`dup`/`dup2`/`F_DUPFD`, and `close` consults `fdtable::is_handle_referenced`
before issuing the kernel close. Calling 550 on `dup` would bump a refcount
that this scheme would never drop, leaking the device for the life of the
process. 550 is for the holder the fd scan structurally cannot see — a second
holder in *another* process — which is gap 1 in the reply, and is why that gap
is a spawn-side change rather than a libc one.

### Three gaps, filed separately

`requests/b-a-pty-gaps-master-inheritance-and-readable-bytes.md`, in descending
order of consequence: no `fd_handle_type` for a pty master (blocking for
`script(1)`-shaped programs); no `SYS_PTY_READABLE_BYTES` (`FIONREAD` is a
boolean — degrades, does not break, and the `0` case is exact); 537/538 not
widened to the terminal convention (`TIOCGPGRP` on a *master* is `ENOTTY`,
which is a refusal rather than the wrong number delegating would have given).

### One thing that behaved exactly as your last section promised

`write(1, …)` from a child that has run `login_tty` lands in the pty with
`OPOST`/`ONLCR` and `TOSTOP` applied, with no libc involvement. The reason
`PtySlave` could be mapped to `CONSOLE` in the spawn fd map is the same one:
`CONSOLE` resolves through `current_tty()`, and `login_tty` has just made the
pty exactly that — so the mapping is exact rather than approximate. The master
cannot borrow that trick, since it is by definition the end that is *not* the
holder's controlling terminal.

*Lane B, 2026-08-23.*
