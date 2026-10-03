### TD-B-PTY-MASTER-HAS-NO-FOREGROUND-GROUP -- FIXED 2026-08-24

**Where:** `posix/src/ioctl.rs`, `is_pgrp_terminal`.

`SYS_TTY_GET_PGRP`/`SYS_TTY_SET_PGRP` (537/538) were not widened to the
terminal-naming convention lane A built for 539 and 553-556, so they still
answer only for the caller's own controlling terminal. Therefore
`TIOCGPGRP`/`TIOCSPGRP` on a pty **master** returns `ENOTTY`. On a
**slave** they delegate to `tcgetpgrp`/`tcsetpgrp` and are correct, because
after `login_tty` the slave *is* our controlling terminal, and when it is
not, `ENOTTY` is the truthful answer.

Delegating on a master would have been the bug worth avoiding: it would
report the *emulator's* foreground process group as though it were the
pty's -- a wrong number instead of a refusal. `is_pgrp_terminal` exists
solely to make that refusal explicit and commented, rather than falling out
of an omission someone later "fixes".

**Proper fix:** 537/538 take a terminal under the same convention as
539/553-556.

**Fixed 2026-08-24 -- but not by that fix, which turns out to be
unimplementable.** New numbers instead: `SYS_PTY_GET_PGRP` = 870,
`SYS_PTY_SET_PGRP` = 871.

The reason the requested shape could not work is worth recording, because it is
not a reason either lane guessed and it generalises to every other syscall
somebody proposes widening:

> **libc invokes 537 as `syscall0`, which never writes `rdi`.**

Giving `arg0` a meaning would therefore not read a zero. It would read whatever
the caller happened to leave in `rdi` -- under the naming convention that is
`0` ("my terminal") sometimes, `1` (reserved, refused) sometimes, and a live
pty handle naming an unrelated terminal the rest of the time. A compatibility
break that fails *nondeterministically*, varying with the caller's register
allocation, is one nobody would ever have diagnosed. 538 has the same problem
one argument along: its `arg0` is the pgid, so the terminal would have to move
to `arg1`, which `syscall1` likewise never writes.

So 537/538 are unchanged and remain correct for the console and the slave, and
`is_pgrp_terminal` still names exactly those two -- its meaning narrowed from
"may the process-group ioctls act on this" to "may they reach it *via
537/538*". A master now takes the other route.

Three properties of the new pair that libc depends on and states at the
constants:

* **`arg0 == 0` is `ENOTTY`, not the console.** Unlike 553-556, "my terminal"
  is not a useful reading: a daemon has no foreground process group, and
  answering with the console's would report a group it has no relationship to
  as its own.
* **A named terminal nobody has claimed is also `ENOTTY`.** A pty whose slave
  has not yet run `TIOCSCTTY` genuinely has no foreground group, and a title-bar
  caller must read that as "nothing is running in there yet" rather than
  receive a `0` it might try to signal.
* **The group is validated against the terminal's session, not the caller's**,
  and `SIGTTOU` follows the terminal rather than the caller. For a master those
  sessions differ by construction, so validating against the caller would be
  simultaneously too strict and too lax -- rejecting every group actually
  running on the pty, and accepting groups from the emulator's own unrelated
  session, which is the terminal-theft case the POSIX rule exists to prevent,
  merely pointed the other way.

Note the argument order differs between the two pairs: 538 takes the pgid as
`arg0`, while 871 takes the terminal as `arg0` and the pgid as `arg1`. libc
rejects a non-positive pgid before the call, since the value is widened into a
`u64` and a negative would sign-extend into an enormous group id.

### The slave cannot be reopened by name after its first claim -- WORKS AS DESIGNED, recorded so it is not mistaken for a bug

**Where:** `posix/src/ptytab.rs`; `open_pty_device` in `posix/src/file.rs`.

On Linux, `/dev/pts/<n>` is a real device node and may be opened any number
of times. Here `SYS_PTY_CREATE` returns *both* ends at once, so libc holds
the slave in `ptytab` between `posix_openpt` and the caller's
`open("/dev/pts/<n>")`, and that open **claims** the held handle rather
than opening a device. A second open of the same name gets `ENOENT`.

This is a consequence of a kernel design decision that is right for other
reasons (it removes Linux's `grantpt` chmod dance and `unlockpt`
`TIOCSPTLCK` state machine entirely -- both are validated no-ops here), and
it costs almost nothing in practice, because the one caller that matters is
`openpty`, which opens the slave exactly once. It is written down here
because a reader who knows Linux will expect the other behaviour and should
find this rather than conclude the claim path is broken.

The holder is also what makes the orphan case safe: a caller who takes a
master and never claims the slave -- precisely what an `openpty` that fails
at `tcsetattr` does -- would otherwise strand a live slave with no
descriptor. `retire_master` reports the orphan on the master's close and
`close_pty_handle` reaps it. Under a Linux-style "open the slave later by
name" design that leak would have been invisible from libc.
