## TD-B-SSHD-RUNS-A-COMMAND-BUT-CANNOT-HOST-A-SESSION — 2026-08-21 — FIXED 2026-09-05

**In short:** `ssh host` — a plain interactive login — used to answer "shell
request failed". It now works: the daemon allocates a pseudo-terminal (a fake
keyboard-and-screen pair that lets a program believe it is at a real terminal),
runs the user's login shell on it as that user, and moves bytes both ways.
Editing, `^C`, job control, `clear`, `vi` and window resizing all behave,
because they are the terminal's job and there is now a real terminal. Two of the
three limitations below are gone; the third — `exec` buffering its output
instead of streaming — remains, and has moved to its own entry.

**Where it was:** `userspace/sshd/src/main.rs` — `handle_channel_request`'s
`pty-req` and `shell` arms, and `handle_channels`.

### What unblocked it

Lane A landed the pty syscalls, fulfilling
`requests/b-a-pty-devices-need-the-line-discipline-that-the-console-already-has.md`
and its two follow-ups. `posix` then grew `openpty`/`login_tty`/`forkpty` on top
of them, which is what a session actually needs: `login_tty` is the only route
by which a child can adopt a terminal as its *controlling* terminal, and a
controlling terminal is what makes `^C` reach the foreground job rather than
nothing at all.

### What was built

**1. `pty-req` allocates a real terminal.** `Pty::open` calls `openpty` with the
client's window size, clamping the 32-bit dimensions SSH sends into the 16-bit
ones `struct winsize` holds — clamping rather than truncating, because a
wrapping cast turns 65 536 columns into *zero* columns, and a zero-width
terminal breaks every line-wrapping program in a way that looks like a bug in
that program. A second `pty-req` on one channel is refused (RFC 4254 §6.2 allows
one, and replacing a live terminal would hang up a running shell).

**2. `shell` runs the login shell on the slave.** `std::process::Command` with a
`pre_exec` closure that calls `login_tty(slave)`; `argv[0]` is the shell's
basename with a leading hyphen, which is the entire protocol by which a shell is
told it is a *login* shell and should read the profiles. Registering `pre_exec`
is also what takes std off its `posix_spawn` fast path — necessary, because
`posix_spawn` has no hook that could acquire a controlling terminal.

The parent then closes its copy of the slave fd. That one `close` is the
difference between a session that ends and one that hangs forever: hangup means
"the last slave closed", so while the daemon holds one, an exited shell leaves a
terminal that never reports the end of the session.

**3. The connection loop became readiness-driven.** `handle_channels` polls the
socket (`SYS_TCP_POLL_STATUS`) and the pty master (`poll` on the fd) and sleeps
with exponential backoff from 0.5 ms to 20 ms when neither has anything. It
keeps a **blocking fast path**: with no child running — during key exchange, all
of authentication, and every `exec`-only session — it blocks on `recv_packet`
exactly as before, so the polling costs nothing on the paths that do not need
it.

Framing was split for this: `try_parse_packet` is a pure function over the
buffer that returns `Ok(None)` for an incomplete packet, and `read_packet` is
the blocking loop around it. The sequence number advances only when a packet is
actually produced — it feeds both the MAC and the CTR keystream, so advancing it
for a packet that had not arrived would desynchronise the cipher permanently.
This is now covered by host tests that feed a packet in one byte at a time.

**4. Flow control follows consumption, not arrival.** Client keystrokes are
queued on the channel and written to the master when it reports writable, and
the SSH window is credited only for bytes that reached the terminal. A program
that stops reading its stdin therefore back-pressures the *client* instead of
being absorbed into the daemon's memory — and, because a `write` to a master
does not honour `O_NONBLOCK` (there is no `SYS_PTY_MASTER_TRY_WRITE`), the
readiness check is what stops one uninterested process from freezing the whole
daemon.

> **Amended 2026-09-06.** The second half of that sentence no longer holds, and
> the readiness check it justified is gone. Lane A built the missing call —
> `SYS_PTY_MASTER_TRY_WRITE` is **1065**
> (`requests/a-b-pty-master-try-write-is-1065-and-it-returns-wouldblock-not-zero.md`)
> — so `posix::write` now routes a master fd carrying `O_NONBLOCK` to it, `Pty`
> sets that flag on the master alongside the pipes, and `pump_channel_input`
> writes without polling first. The poll was never able to do the job claimed
> for it: it reported that there *was* room, and another writer on a dup'd
> master could take it before the write landed. The check and the transfer now
> happen under one lock inside the kernel, so the write's own return is the only
> non-racy reading of the destination.
>
> One thing had to be fixed in the same change or the flag would have been
> actively harmful: `Pty::write_input` treated every negative return as a fatal
> error, and a terminal's input half failing tears down the whole session
> because one descriptor carries both directions. That was safe only while
> `EAGAIN` was unreachable there. With the flag set, a shell that had merely
> stopped reading for a moment would have been read as a dead session and hung
> up mid-keystroke. It now returns `Ok(0)` for `EAGAIN`, exactly as the pipe
> path always has.

**5. Ending a session waits for both halves.** The channel closes only once the
process has exited *and* the terminal has run dry — tracked as two separate
facts — so a shell's final `logout`, or the last screen a full-screen program
painted, is not cut off by the close. Hangup on the master drops the terminal
but deliberately does *not* invent an exit status; the status is whatever `wait`
reports, because a fabricated one would tell a caller's `if ssh host cmd; then`
the wrong thing.

**6. Channel teardown became once-only, and now takes the session with it.**
Long-lived sessions turned two latent framing faults into reachable ones, so
both were fixed at their source rather than at the new call site:

- `send_channel_eof` now takes the channel's *local* id (like
  `send_channel_close`) and owns the `eof_sent` flag. Three call sites used to
  test-and-set that flag independently and two skipped it, so the natural
  `send_channel_eof(...); send_channel_close(...)` pairing — written by every
  session-ending path in the file — emitted **two** `SSH_MSG_CHANNEL_EOF` per
  channel. `send_channel_close` is likewise a no-op on an already-closed
  channel, which is RFC 4254's rule: each side sends `CHANNEL_CLOSE` once and
  the other replies. Without that, the ordinary end of an interactive session —
  we close, the client closes back — sent a second close for a dead channel.
- `handle_channel_close` now drops the pty and kills and reaps the child.
  Before, it only set a flag, and the connection loop skips closed channels: a
  client closing its channel without waiting (which is exactly what `~.` does)
  would have stranded a running shell holding a pty for the lifetime of the
  daemon, with no code path left that could ever notice it. Dropping the master
  hangs the terminal up so a cooperative shell exits on its own; the kill is
  the half that does not depend on the shell cooperating.

### Still open, moved to its own entry

`exec` still collects output and sends it when the command exits, and `shell`
without a `pty-req` (`ssh -T host`) is still refused. Both need the same thing —
the pump extended over a child's ordinary pipes as well as a pty master — and
are tracked as `TD-B-SSHD-EXEC-DOES-NOT-STREAM-AND-SSH-DASH-T-IS-REFUSED`.

The smaller gaps below were not addressed and carry over to that entry:

- **`env` requests are accepted and discarded.** The arm replies SUCCESS
  without recording anything, so `ssh -o SendEnv=LC_ALL host` silently loses
  the variable. Consistent with OpenSSH, which also ignores unlisted variables
  silently, but not yet a real implementation.
- **Supplementary groups are not set.** The session sets the primary gid and
  uid; `setgroups` is not called, so a session does not carry the account's
  secondary group memberships. Blocked on the same gap `su` and `doas` have —
  see the comment at `userspace/doas/src/main.rs:731`.
- **The client's terminal modes are not applied.** `pty-req` carries the
  client's `termios` settings and they are dropped. They describe the *client's*
  terminal, and the shell reconfigures the terminal immediately on startup
  anyway, so the practical effect is nil; OpenSSH applies them only because it
  carries a `termios` translation table this daemon does not have.
