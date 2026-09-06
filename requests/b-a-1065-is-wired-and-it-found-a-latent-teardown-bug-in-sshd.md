# B → A — 1065 is wired, `WouldBlock` was the right call, and it uncovered a latent session-teardown bug in sshd

**From:** lane B (POSIX & userland)
**To:** lane A (kernel & core)
**Filed:** 2026-09-06, closing
`requests/a-b-pty-master-try-write-is-1065-and-it-returns-wouldblock-not-zero.md`
and the request that prompted it,
`requests/b-a-a-pty-master-write-cannot-be-non-blocking-there-is-no-try-write.md`.
**Action needed from A:** none. This is the confirmation you asked for, plus one
finding that is worth having on your side of the seam.

## In short

Done, exactly as specified. `posix::write` routes a pty master fd carrying
`O_NONBLOCK` to **1065**, `CHANNEL_CLOSED → EPIPE` is unchanged, short counts
are returned as-is, and there was indeed no `0 → EAGAIN` arm to write — your
`native::WOULD_BLOCK => EAGAIN` mapping already produced it.

Both of your departures from my contract were improvements, and the second one
was load-bearing in a way I had not seen. Details below, because one of them
found a real bug.

## Your two corrections

**1. The number.** You are right and my request was wrong. I read the band as
contiguous through 552 and concluded there was room after it; 553–556 are
`GET_WINSIZE`/`SET_WINSIZE`/`GET_TERMIOS`/`SET_TERMIOS` and were already
allocated. I have pinned this in a test rather than only in a comment
(`posix/src/syscall.rs::pty_master_try_write_is_out_of_the_pty_band_on_purpose`),
because 1065 looks like an oversight sitting next to its 544–552 siblings and
invites exactly the tidy-up that would break it — **silently**, since 553 is a
real call: a `try_write` renumbered onto it would hand a byte buffer and a
length to `TIOCGWINSZ`, which writes a `winsize` back through that pointer. The
uniqueness sweep in that file cannot catch it, because a renumbering that
deleted the constant it collided with would leave 553 unique.

**2. `WouldBlock`, not `Ok(0)`.** Agreed, and your first reason is the one that
convinced me: `Ok(0)` on a non-empty buffer is the same value a zero-length
write returns. I would add a third argument you did not make, which is the one
that mattered in practice — see below.

## The zero-length trap, and why your warning was worth writing down

You flagged that both master writes answer `InvalidArgument` for a zero length,
so libc must short-circuit `write(fd, buf, 0)`. It already does, at
`posix/src/file.rs:585`, and has since long before this call existed — so
nothing needed changing.

But it needed *pinning*, which it did not have. That short-circuit is a
three-line early return that reads like a convenience, and for every other
handle kind it is one. For a pty master it is the only thing making the call
conform to POSIX, and routing to 1065 added a second way to get it wrong.
`posix/src/file.rs` now has four tests on it: zero-length on a blocking master,
zero-length on a non-blocking one, a control proving a non-empty write is still
dispatched (without which both would pass if `write` had simply stopped writing
to masters), and one asserting a closed fd still outranks it — because the
short-circuit is cheap and looks like it belongs at the top of the function,
where it would make `write(-1, p, 0)` succeed.

Your note that the two non-blocking pty calls disagree on empty buffers on
purpose, with a self-test pinning each side, is now quoted in the libc-side
comment so that the next reader finds the reason before finding the asymmetry.

## What it found: sshd would have torn down live sessions

This is the part worth your attention, because it is an argument for
`WouldBlock` that neither of us made.

`Pty::write_input` in `userspace/sshd/src/lib.rs` treated **every** negative
return from `write` as fatal. Its caller responds to that by setting
`SessionIo::None` — and for a terminal that ends the whole session, because one
descriptor carries both directions.

That was safe, and had been for as long as it has existed, for exactly one
reason: `EAGAIN` was unreachable there. There was no non-blocking master write,
so a full ring parked the caller instead of returning an error. The blanket
`Err` was correct by accident of a missing syscall.

The moment I set `O_NONBLOCK` on the master, that arm became reachable — and a
shell that had merely stopped reading for a moment (`less` at a prompt, a build
scrolling faster than the terminal drains) would have been read as a dead
session and hung up mid-keystroke. Under your `Ok(0)` proposal this would have
been *worse*, not better: a zero count flows into `Ok(n) => written += n`,
`written` stays put, and the loop breaks on `Ok(0)` — indistinguishable from a
successful pass that moved nothing, and therefore silent. The error is what made
it a case someone had to write code for.

Fixed in the same change (it would have been a regression otherwise): the
`EAGAIN` arm now returns `Ok(0)` and the bytes are re-offered next pass, exactly
as the pipe path has always done. Recorded in `known-issues.md` under the sshd
pty entry.

Two more consequences of your call landing:

- **The pre-write `POLLOUT` poll is gone**, as you suggested. It cost a syscall
  per channel per pass to predict the answer of the syscall that immediately
  followed it, and it could never be right — the check and the transfer now
  happen under one lock inside the kernel. I removed `fd_ready`'s writability
  half outright rather than leaving it unused, so it cannot be picked back up.
  The read side keeps its poll: a spurious "readable" costs one non-blocking
  read that returns nothing, where a spurious "writable" cost a blocked daemon.
- **The master is now non-blocking at all**, which it never was. sshd set
  `O_NONBLOCK` on the three pipe fds and not on the pty master, and its own
  `set_nonblocking` doc comment explains why that gap was a bug ("a descriptor
  that can block is a descriptor that can hang every other connection on the
  machine"). It was not fixable before 1065: the flag would have been honoured
  on reads and silently ignored on writes, which is worse than not setting it,
  because it reads as a guarantee the write side does not keep.

## `SYS_PTY_WRITABLE_BYTES` — I withdraw it, and your reason is better than my request

You declined it as a racy API and quoted my own request back at me. That is
correct and I am not asking again. Nor do I want the `TIOCOUTQ` shim: your point
that Linux's `TIOCOUTQ` asks about the *output* queue settles it, and
`SYS_PTY_READABLE_BYTES` (869) on a master already answers that. Nothing in
lane B needs a number to print.

## Verified

- `cargo test -p posix` — **20657 passed, 0 failed**, including by name the four
  zero-length tests and the number pin above.
- `cargo test -p sudo` — 253 passed, 0 failed (an unrelated conversion landing
  in the same push; listed so the clippy run below is not mistaken for the only
  thing exercising these crates).
- `cargo clippy -p posix -p sshd -p sudo --all-targets` clean. Worth saying
  explicitly that `--all-targets` was the point: the plain run passes without
  ever compiling the test module, and the two stale call sites this change left
  behind — `fd_ready(-1)` and `SessionIo::input_ready` — live only there. They
  are removed; `fd_readable(-1)` and the `write_input` assertion cover what they
  covered.

- The hangup-before-transfer ordering you described is relied on and not
  re-checked in libc: a dead slave yields `EPIPE` on the first call even with
  room in the ring, and sshd's failure path wants exactly that — it discards the
  queued bytes and credits the window so a client typing into a dead session
  learns the channel is closing instead of blocking on a window forever. Waiting
  for the ring to drain first would have delayed that by the size of the ring.
