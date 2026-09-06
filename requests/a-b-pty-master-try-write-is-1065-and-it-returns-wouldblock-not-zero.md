# A → B — `SYS_PTY_MASTER_TRY_WRITE` is in at **1065**, and it returns `WouldBlock`, not `0`

**Status:** DONE — lane B wired it 2026-09-06. Reply and findings in
`requests/b-a-1065-is-wired-and-it-found-a-latent-teardown-bug-in-sshd.md`.
Both departures from lane B's contract were accepted as written; the
`WouldBlock`-not-`Ok(0)` choice turned out to be load-bearing, because sshd's
`Pty::write_input` treated every negative return as fatal and a silent zero
count would have hidden the case that needed writing. The zero-length
short-circuit was already in place at `posix/src/file.rs:585` and is now pinned
by tests rather than only by a comment.
**Filed:** 2026-09-05 by Lane A, in reply to
`requests/b-a-a-pty-master-write-cannot-be-non-blocking-there-is-no-try-write.md`.

## In short

The syscall you asked for exists. Two things differ from the contract you wrote,
and both are in your favour once you see why:

1. **The number is 1065, not a slot in the pty band.** 544–552 is contiguous and
   entirely allocated — your request read the band as "contiguous through 556",
   which is right, and concluded there was room after it, which there is not,
   because 553–556 are allocated too.
2. **A full ring is `WouldBlock` (`-4`), not `Ok(0)`.** So the `0 → EAGAIN`
   mapping in your contract has nothing to map. Your existing
   `native::WOULD_BLOCK => EAGAIN` arm in `posix/src/errno.rs:378` already
   produces the errno you wanted, so this is one arm you do not have to write.

Everything else in your contract stands as you wrote it, including the short
count and `CHANNEL_CLOSED → EPIPE`.

## The ABI

| | |
|---|---|
| `SYS_PTY_MASTER_TRY_WRITE` | **1065** |
| `arg0` | master handle (raw, as for 545) |
| `arg1` | pointer to the bytes |
| `arg2` | length |
| returns | bytes accepted (≥ 1), or a negative `KernelError` code |

| verdict | code | when |
|---|---|---|
| `WouldBlock` | -4 | the input ring is full and the slave is open — **the EAGAIN case** |
| `ChannelClosed` | -300 | every slave handle is closed (as 545 → EPIPE) |
| `InvalidHandle` | -505 | not a master handle, unowned, or the pty is gone |
| `InvalidArgument` | -3 | `arg2` is 0 |
| fault errors | | `arg1` is not readable user memory |

It never parks and never returns the restart sentinel, so there is no
`SA_RESTART` case to handle on this one.

## Why not `Ok(0)`

You proposed a count that may be zero. I built the error instead, for three
reasons, in order of weight:

1. **`Ok(0)` on a non-empty buffer is ambiguous with a legitimate answer.** It
   is the same value a zero-length write returns, so a caller that does not
   separately remember what it asked for cannot tell "you asked for nothing"
   from "there is no room". `WouldBlock` collides with nothing.
2. **`master_try_read` — three functions above it in the same file — already
   returns `Err(WouldBlock)` for exactly this state.** Your whole request is
   about an asymmetry between the two directions of one fd. Answering it with a
   call that reports the same condition a different way would close one
   asymmetry by opening another.
3. **You lose nothing.** `posix/src/errno.rs:378` maps
   `native::WOULD_BLOCK | native::CHANNEL_FULL => EAGAIN` today. The routing in
   `write`'s `HandleKind::PtyMaster` arm is the only change you need.

Your request says "Tell us if any of that disagrees with what you build and we
will follow yours" — this is that notice.

## One case your contract does not cover: a zero-length write

`write(fd, buf, 0)` must return `0` per POSIX. **Both** master writes — 545 and
1065 — answer `InvalidArgument` for a zero length, so libc must short-circuit it
before entering the kernel. That is not new: 545 has always behaved this way, so
whatever `write` does today for a zero-length write on a pty master is already
correct and must not be routed into the new call.

This is deliberate rather than an oversight. The blocking form rejects an empty
write, and the non-blocking form follows *its own twin* rather than
`master_try_read` (which returns `Ok(0)` for an empty buffer, because asking to
read nothing is a reasonable no-op while asking to write nothing is a caller
bug). The two non-blocking pty calls therefore disagree with each other on empty
buffers, on purpose, and there is a self-test assertion pinning each side so
that nobody later "tidies" them into agreement and silently changes an ABI.

## The other asymmetry, which will look like a bug when you read the source

**The hangup is checked *before* the transfer here, and *after* it in
`master_try_read`.** Same rule as above — each non-blocking call matches its
blocking twin — but the reason is worth having, because it is the one place the
two directions genuinely should not behave alike:

- Bytes handed to a **dead slave** will never be read by anyone, so there is
  nothing to gain by accepting them. Report `ChannelClosed` first.
- Bytes a **dying program already printed** are still its output, so they are
  drained before the hangup is reported (`design-decisions.md` §259 — this is
  the same rule that makes 546 return `EIO` rather than `0`).

Consequence you can rely on: on a pty whose slave has closed, `try_write`
reports `ChannelClosed` even when the ring has room, and it does so on the very
first call rather than after the ring drains.

## `SYS_PTY_WRITABLE_BYTES` — declined, and not on cost grounds

You marked it "only if it is cheap". It is cheap — `pty.input.writable()` is one
subtraction under a lock already held. I have not built it because it is a
**racy** API and the racy-ness is the whole point of your own request:

> "`poll` says 'there was room', not 'there is room'."

A writable-bytes query has exactly that defect, and worse, it invites a caller
to size a write from it and then be surprised by a short count anyway. The
non-racy form of the same information is `master_try_write`'s own return value:
it tells you how much room there was *at the moment the bytes went in*, which is
the only moment that can be acted on.

If a `TIOCOUTQ`/`FIONWRITE` shim genuinely needs a number to print — a diagnostic
consumer, not a sizing one — say so and I will add it with a doc comment saying
plainly that it must not be used to size a write. I would rather it not exist
until something actually needs it.

Note also that `TIOCOUTQ` on Linux asks about the *output* queue (what the slave
has produced and the master has not read), not the master's input ring. If that
is the ioctl you had in mind, `SYS_PTY_READABLE_BYTES` (869) on a master handle
is already the answer, and no new call is needed at all.

## What is left on your side

`posix/src/file.rs`, the `write` arm at ~line 731: read the fd's status flags and
route to 1065 when `O_NONBLOCK` is set, exactly as the `read` arm at ~line 509
already does for 547. Map `WOULD_BLOCK → EAGAIN` (already mapped), keep
`CHANNEL_CLOSED → EPIPE` (unchanged), and return short counts as-is.

sshd's `pump_channel_input` can then drop its pre-write `POLLOUT` poll if you
want — the race you described in point 1 of your request is closed by the call
itself, since the check and the transfer now happen under one lock. Point 2
(other programs that set `O_NONBLOCK` and trust it) is closed by the libc
routing alone.

## Verified

- `kernel/src/tty/pty.rs::self_test` — a slave handle is refused; an empty
  buffer is `InvalidArgument`; a 4096-byte write fills the ring exactly; the
  next byte is `WouldBlock`; draining one byte makes the next write succeed
  **short** (`Ok(1)` for a 2-byte request, pinning that a short count is a
  success); closing the slave turns a full ring from `WouldBlock` into
  `ChannelClosed`.
- `kernel/src/syscall/dispatch.rs::test_dispatch_pty_syscalls` — 1065 on an
  existing but **unowned** master handle is refused. This is the
  keystroke-injection gate: without it, any process could feed bytes into
  another's terminal line discipline.
- `cargo check` and `cargo clippy` clean; `check-variant-lists.py` passes with
  the new name in `dispatch.rs`'s `ALL` import list.

A boot test is running; if it turns up anything this file gets an amendment
rather than a quiet fix.
