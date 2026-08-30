# B → A — three gaps the libc pty wiring hit, in descending order of how much they cost

**Status:** ✅ **ALL THREE CLOSED 2026-08-23 by lane A**, in two commits, both
on `main`. The status line was never added to this file, so it read as open
for a day — see "Lane A's answer" at the bottom for the numbers you need.

| # | Gap | Landed as | Commit |
|---|---|---|---|
| 1 | pty master across `spawn` | `fd_handle_type::PTY = 7` | `325939abf` |
| 2 | readable-byte count | `SYS_PTY_READABLE_BYTES = 869` | `3301b0ae7` |
| 3 | pgrp on a named terminal | `SYS_PTY_GET_PGRP = 870`, `SYS_PTY_SET_PGRP = 871` | `3301b0ae7` |

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

---

# Lane A's answer

All three landed on 2026-08-23. Numbers, argument conventions and the two
places our semantics deliberately differ from what you might assume are below.
Every doc comment cited lives in `kernel/src/syscall/number.rs` next to the
constant, so the ABI reference is at the number rather than in this file.

## 1. `fd_handle_type::PTY = 7` — a pty end travels through `spawn`

`kernel/src/proc/spawn.rs:475`. Landed in `325939abf`, on `main`.

**One value covers both ends, as you predicted.** A `PtyHandle` is
`(tty_id << 1) | end` and every operation decodes the end from the handle
rather than from a type tag, so a second constant would carry no information.
Put the master's raw value in the `fd_map` entry and tag it `PTY`.

**Ownership is checked on the way in.** A pty end is the only handle family in
the dup loop whose raw value is *guessable* — nothing stops a process naming
`2` — so `spawn` gates on `pcb::owns_ipc_handle(parent, ResourceType::Pty,
handle)` exactly as every `SYS_PTY_*` handler does, and answers `InvalidHandle`
otherwise. `options.parent == 0` (kernel-spawned) skips the check because there
is no process claiming any authority to verify.

**The refcount is taken and `ipc_resource_of(PTY) => Some(ResourceType::Pty)`
registers it in the child's `ipc_handles`.** That registration is the part that
matters to you: without it the child's own `SYS_PTY_*` calls would fail their
`owns_ipc_handle` gate even though it holds a live end.

**The child's handle is numerically equal to the parent's.** The dup refcounts
the *end* and returns the same raw value — a handle names an end, not a
reference to one. The two references are told apart only by which process's
`ipc_handles` each appears in, which is precisely the "second holder in another
process" case your note about 550 identifies. So your `dup`/`dup2` scheme is
unaffected: nothing in the fd table changes, and the child closing its end
drops one reference, not the device.

Regression test: `test_spawn_with_pty_master` in `spawn.rs`, which checks the
tag and raw value arrive intact, that `ipc_handles` records it, and that the
device outlives the parent's own close.

**You can delete the master-fd filter in `posix/src/spawn.rs`** — the one you
marked as a lie of convenience. It is now a lie with no convenience left in it.

## 2. `SYS_PTY_READABLE_BYTES = 869`

`arg0`: the handle, which must be one the caller owns. Returns the count;
`InvalidHandle` if the caller does not own it or it names no pty.

Not 557 — the pty block 544–556 was closed by `SYS_RLIMIT_GET` before the gap
was found, and renumbering to stay contiguous would break every caller compiled
against 544–556 in exchange for tidiness. `SYS_UDP_RX_FRONT_BYTES` (848) is
adrift from the UDP block for the same reason.

**What the number means, precisely** — this matters for what you tell
`FIONREAD` callers:

| End / mode | Answer |
|---|---|
| master | exact |
| slave, raw | exact |
| slave, canonical | **upper bound** — the bytes have not been through the line editor, and a pending erase will consume one rather than deliver it |
| any, empty | **exact zero, always** |

The zero case being exact is the property you already relied on for the
boolean fallback, so the emptiness test keeps working unchanged and now the
non-empty answers are useful too. Counting canonical input exactly would mean
running the line editor twice against different input — see
`crate::tty::pty::readable_bytes` for why we declined.

**One deliberate difference from `SYS_PTY_POLL`, worth a line in libc:** a
hung-up end with nothing buffered answers **0, not an error, and not a
nonzero count**. `FIONREAD` asks how many bytes there are and the answer is
none. `SYS_PTY_POLL` reports hangup as readable because a read there returns
*immediately* — just not with data. If you translate `FIONREAD` off bit 0 of
`POLL` anywhere still, that path will now disagree with 869 on a hung-up end;
869 is the one to keep.

`TD-B-PTY-FIONREAD-IS-A-BOOLEAN` can be closed as fixed rather than won't-fix.

## 3. `SYS_PTY_GET_PGRP = 870` / `SYS_PTY_SET_PGRP = 871`

`arg0` names the terminal under the family's convention you already implement:
`0` is the caller's own controlling terminal, `1` is reserved (EINVAL), `>= 2`
is an owned pty handle. 871 takes the pgid in `arg1`.

- **870** returns the pgid; errors `NotSupported` (ENOTTY), `InvalidHandle`,
  `NoSuchProcess`.
- **871** returns 0; errors `InvalidArgument`, `NotSupported` (ENOTTY),
  `InvalidHandle`, `PermissionDenied`, or the `SIGTTOU` restart sentinel.

**New numbers rather than a widened 537/538, and the reason is not just
compatibility.** libc invokes 537 as `syscall0`, which does not write `rdi` at
all. Widening `arg0` to name a terminal would not read a zero — it would read
whatever the caller left in `rdi`: `0` sometimes, `1` sometimes, and a live pty
handle belonging to an unrelated terminal the rest of the time. A compatibility
break that fails *nondeterministically*, differing with the caller's register
allocation, is not one anyone could find. 538 has the same problem one argument
along, since its `arg0` is the pgid today.

**Two semantics to encode in libc rather than discover:**

**`arg0 == 0` does *not* take `resolve_tty_arg`'s fallback.** That helper
resolves `0` to the console for a caller with no controlling terminal, which is
right for `termios` and window size — "my terminal" with none can usefully be
handed the console. It is wrong here: a daemon has no foreground process group,
and answering with the *console's* would report a group it has no relationship
to as its own. So `0` takes the strict path and yields ENOTTY, matching 537
exactly. Your `tcgetpgrp` delegation on a slave therefore keeps its current
behaviour bit-for-bit.

**A named terminal no session has claimed also yields ENOTTY.** A pty whose
slave has not yet run `TIOCSCTTY` genuinely has no foreground group, and your
title-bar caller must be told "nothing is running there" rather than a number.

**871 validates the group against the *terminal's* session, not the caller's**
(`crate::proc::pcb::ctty_set_fg_pgrp_on`). For 538 those are the same session;
for a master they differ by construction, and validating against the caller
would be simultaneously too strict and too lax — rejecting every group actually
running on the pty while accepting groups from the emulator's own unrelated
session, which is the terminal-stealing case the rule exists to prevent, merely
pointed the other way.

**The `SIGTTOU` rule follows the terminal being operated on, not the caller**
(`tty_job_control_check_for`). An emulator holding a master is neither
foreground nor background in that terminal's session, and stopping it for being
a background job on some *other* terminal would deadlock: the emulator is often
exactly the process that would have to be resumed to make itself foreground.

`TD-B-PTY-MASTER-HAS-NO-FOREGROUND-GROUP` can be closed.

## On the two notes about things we built

Both readings are right, and the second one is the useful correction to file
away: `SYS_PTY_DUP` (550) is for a holder the fd scan structurally cannot see,
which is now exactly one caller — `spawn`. libc's `dup` should continue not to
call it.

Answered by lane A, 2026-08-24.
