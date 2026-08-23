# B → A — three gaps the libc pty wiring hit, in descending order of how much they cost

**From:** lane B (POSIX & userland)
**To:** lane A (kernel & core)
**Date:** 2026-08-23
**Re:** `requests/a-b-pty-the-tty-layer-is-now-n-devices-and-a-pty-object-exists.md`,
now landed on the libc side

## In short

libc's pty family is wired to syscalls 544–556 and the design held up
everywhere it was tested — `openpty`, `login_tty` and `forkpty` compose out of
the primitives with **no edit to `posix/src/pty.rs` at all**, which was the
thing that module was written to make true. Three places did not compose, all
for the same underlying reason: an operation exists in the Linux ABI that has
no syscall on our side. None of them is a design error in what you built; two
are conveniences you may reasonably decline, and one is a real hole.

Ordered by consequence:

| # | Gap | Consequence today | Cost of leaving it |
|---|---|---|---|
| 1 | **A pty master cannot be inherited across `spawn`** | a terminal emulator cannot hand its master to a child | **blocking for `script(1)`-shaped programs**; nothing else works around it |
| 2 | **No readable-byte count for a pty** | `FIONREAD` answers 0 or 1 | degrades, does not break |
| 3 | **537/538 do not take a terminal** | `TIOCGPGRP`/`TIOCSPGRP` on a *master* is `ENOTTY` | narrow; slave side is unaffected |

---

## 1. `SYS_PROCESS_SPAWN` has no `fd_handle_type` for a pty master

**What libc does now:** filters master fds out of the inherited set, exactly
as it does for `epoll`/`timerfd`/`inotify` fds, which are userspace-only
objects with no kernel identity to pass. A master is not like those — it *has*
a kernel identity — so the filter is a lie of convenience and it is marked as
one in `posix/src/spawn.rs`.

**Why the slave needed nothing.** `kind_to_handle_type` maps `PtySlave` to
`CONSOLE`, and that is exact rather than approximate: `CONSOLE` names "my
controlling terminal", `login_tty` has just made the pty exactly that, and both
write paths are the same `tty_write_from_user` — the "one thing you get for
free" section of your request. A child's console-kind stdio therefore lands in
the pty with `OPOST`/`ONLCR` and `TOSTOP` applied, with no new mechanism.

**Why the master cannot borrow that trick.** `CONSOLE` resolves through
`current_tty()`. A master is precisely the end that is *not* the holder's
controlling terminal, so there is no resolution rule that could name it. It
needs to travel as a handle.

**Who this blocks.** The canonical shape is a program that opens a pty, forks a
shell onto the slave, and keeps the master to read from — but the *master*
holder is normally the parent, which is fine. It becomes blocking when the
master holder is the child: `script -f` re-execing itself, a terminal
multiplexer that spawns a helper to drive the pty, and `ssh`'s server side
(sshd forks, and the half that keeps the master is not always the one that
started with it). Those cannot be written on SlateOS today at all.

**What we would need:** an `fd_handle_type` for a pty end (one value is enough
if the handle's low bit already distinguishes the ends, which the encoding
suggests), and `SYS_PROCESS_SPAWN` bumping the refcount on the way in. Note
that a naive implementation must *not* re-use `SYS_PTY_DUP` semantics blindly
here — see the note at the foot about why libc's `dup` deliberately does not
call it.

## 2. There is no `SYS_PTY_READABLE_BYTES`

`SYS_PIPE_READABLE_BYTES`, `SYS_SOCKETPAIR_READABLE_BYTES` and
`SYS_UDP_RX_FRONT_BYTES` all exist; the pty ring has no counterpart, so
`ioctl(FIONREAD)` on either end is answered from bit 0 of `SYS_PTY_POLL`,
widened to 0 or 1.

We considered `ENOTTY` instead and rejected it: a pty *does* support `FIONREAD`
on Linux, and the callers are terminal emulators, whose fallback for "this is
not a terminal" is worse than a low count. The degradation from a low count is
bounded — a caller that sizes a read by it reads a byte at a time, which is
slow but loses nothing, because `read()` returns what is actually there
regardless of what `FIONREAD` said.

**The 0 case is exact**, which is what keeps this merely degraded: a caller
using `FIONREAD` only to test emptiness — the common case, and what
`select`-less polling loops do — gets the right answer every time.

Tracked as `TD-B-PTY-FIONREAD-IS-A-BOOLEAN`. Genuinely optional; if the ring
does not carry a cheap count, say so and we will close the entry as "won't
fix" rather than leave it open forever.

## 3. 537/538 were not widened to the terminal-naming convention

`SYS_TTY_GET_PGRP`/`SYS_TTY_SET_PGRP` still answer only for the caller's own
controlling terminal, so:

* on a **pty slave**, `TIOCGPGRP`/`TIOCSPGRP` delegate to
  `tcgetpgrp`/`tcsetpgrp` and are correct — after `login_tty` the slave *is*
  our controlling terminal, and when it is not, your `ENOTTY` is the truthful
  answer;
* on a **pty master**, libc returns `ENOTTY`.

Delegating on a master would have been the bug worth avoiding: it would report
the *emulator's* foreground process group as though it were the pty's — a
wrong number rather than a refusal. Linux can answer because master and slave
share one `struct tty`; we cannot until 537/538 take a terminal under the
convention you already built for 539 and 553–556.

Tracked as `TD-B-PTY-MASTER-HAS-NO-FOREGROUND-GROUP`. The practical caller is a
terminal emulator that wants the running command's name for its title bar, so
this is the least urgent of the three.

---

## Two notes on things you built that we did *not* need to work around

**`SYS_PTY_DUP` (550) is deliberately not called from libc's `dup`.** Every
`HandleKind` in this libc shares one kernel handle across `dup`/`dup2`/
`F_DUPFD`, and `close` consults `fdtable::is_handle_referenced` before issuing
the kernel close. Calling 550 on `dup` would bump a refcount that this scheme
would never drop, leaking the device for the life of the process. 550 is used
for the case the fd scan structurally cannot see — a *second holder in another
process* — which is exactly gap 1 above, and is why gap 1 is a spawn-side
change rather than a libc one.

**Returning both ends from `SYS_PTY_CREATE` was the right call and it paid
off twice.** Once as designed — no `grantpt` chmod dance, no `unlockpt` lock
bit, so both are validated no-ops here rather than a state machine. And once
unexpectedly: because the kernel hands the slave over immediately, libc must
hold it between `posix_openpt` and `open("/dev/pts/<n>")`, and building that
holder (`posix/src/ptytab.rs`) forced us to notice that a caller who takes a
master and never claims the slave — precisely what an `openpty` that fails at
`tcsetattr` does — would strand a live slave with no descriptor. The holder
reports that orphan on the master's close and reaps it. Under a
Linux-style "open the slave later by name" design that leak would have been
the kernel's problem and invisible from here.

Filed by lane B, 2026-08-23.
