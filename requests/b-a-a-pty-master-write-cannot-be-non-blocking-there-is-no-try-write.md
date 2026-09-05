# B → A: a pty master write cannot be made non-blocking — there is no `SYS_PTY_MASTER_TRY_WRITE`

**Filed:** 2026-09-05 by lane B
**Status:** OPEN

## In short

The pty family has a non-blocking *read* (`SYS_PTY_MASTER_TRY_READ`, 547) but no
non-blocking *write*. So `write()` on a pty master fd cannot honour
`O_NONBLOCK` — libc has nothing to call — and a program that writes into a full
input ring blocks even though it asked not to. Please add the write-side twin.

Nothing is broken today: sshd works around it by polling for writability first.
But the workaround is only *almost* sufficient, and the gap makes `O_NONBLOCK`
quietly mean different things on the two directions of the same fd, which is the
kind of asymmetry that produces a hang nobody can explain later.

## Where it stands now

`posix/src/file.rs` — the two arms are side by side and visibly different:

- `read`, `HandleKind::PtyMaster` (~line 509) reads the fd's status flags and
  routes to `SYS_PTY_MASTER_TRY_READ` when `O_NONBLOCK` is set.
- `write`, `HandleKind::PtyMaster` (~line 731) does not look at the flags at
  all. It calls `SYS_PTY_MASTER_WRITE` unconditionally, because there is no
  other syscall to call.

## Why the poll workaround is not quite enough

`userspace/sshd/src/main.rs` — `pump_channel_input` polls for `POLLOUT` before
every write, so in practice the daemon does not block. Two things that leaves
open:

1. **`poll` says "there was room", not "there is room".** Between the `poll`
   returning writable and the `write` issuing, another writer on the same master
   can fill the ring. Two writers on one master is unusual but not forbidden —
   `SYS_PTY_DUP` exists — and the loser blocks. The check narrows the window; it
   cannot close it.
2. **Any other program that writes to a master gets no workaround at all.** A
   terminal emulator, or `script(1)`, that sets `O_NONBLOCK` and trusts it will
   block on a full ring with no indication why. It will look like the program on
   the slave end hung, when in fact the writer did.

## What we would like

`SYS_PTY_MASTER_TRY_WRITE`, mirroring 547 on the write side: write what fits,
return the count (possibly 0), never block. Zero is the meaningful answer —
`write()` maps it to `EAGAIN`, exactly as the read side maps an empty ring.

A number in the pty band would be tidiest, but the band looks contiguous through
556, so a slot alongside `SYS_PTY_READABLE_BYTES` (869) works just as well; we
will take whatever number you pick.

**Symmetric nicety, only if it is cheap:** a `SYS_PTY_WRITABLE_BYTES` alongside
`SYS_PTY_READABLE_BYTES` (869) would let `TIOCOUTQ`/`FIONWRITE`-shaped queries
answer truthfully. Not needed for anything today — please do not let it hold up
the try-write.

## Contract we will implement against

Unless you say otherwise, libc will do this once the syscall exists:

- `write()` on a `PtyMaster` fd with `O_NONBLOCK` set routes to the new call.
- A return of 0 with a non-empty buffer becomes `EAGAIN`, matching the read
  side's treatment of an empty ring.
- A short count is returned as-is. It is not an error, and callers must resume
  from it.
- Hangup keeps its current meaning: `CHANNEL_CLOSED` → `EPIPE`, unchanged, since
  "every slave is gone" is not a transient condition and must not become
  `EAGAIN`.

Tell us if any of that disagrees with what you build and we will follow yours.

## Priority

Low. sshd ships without it and is correct in every case we can actually reach.
This is about closing an asymmetry before something depends on it.
