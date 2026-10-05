## B-SSHD-EXEC-REPLIED-WITH-THE-COMMAND-INSTEAD-OF-RUNNING-IT — 2026-08-21 — FIXED

**In short:** `ssh host 'cat /etc/passwd'` used to come back with the text
`exec: cat /etc/passwd` and an exit code of 0. Nothing ran. The output looked
enough like output that a script could not tell, and the success was invented
too — so `ssh host false` also said everything was fine. `exec` now really runs
the command, as the authenticated user, and reports its real stdout, stderr and
exit status.

**Where it was:** `userspace/sshd/src/main.rs`, `handle_channel_request`'s
three session arms.

### What was wrong

sshd is a protocol-complete SSH-2 server — real Diffie-Hellman key exchange,
AES-128-CTR, HMAC-SHA256, password and public-key auth, channel windows, the
lot. Underneath that, the session layer fabricated all three of its answers:

| Request | Old behaviour | Why that is worse than not implementing it |
|---|---|---|
| `exec` | replied SUCCESS, then sent `format!("exec: {cmd}\r\n")` as channel data | the client cannot distinguish echoed input from real output |
| `shell` | replied SUCCESS, sent `Welcome to Slate OS, {user}!\r\n$ ` and echoed keystrokes | looks like a shell at the terminal; there is no process |
| `pty-req` | replied SUCCESS, allocated nothing | client puts *its* terminal in raw mode and waits for echo forever |

And no `exit-status` was ever sent on any path. RFC 4254 §6.10 makes that
request the only way a server reports how a command ended; the OpenSSH client
treats a channel that closes without one as **success**. So every failure — a
command that did not exist, a command that ran and returned 1, a command that
was never run at all — arrived at the caller as exit 0. `roadmap.md` marked
"PTY support, session management" as `[x]` on the strength of the messages
being *parsed*.

### The fix

- **`exec` executes.** `run_exec_request` resolves the authenticated username
  in `/etc/passwd`, spawns `<login shell> -c '<command>'` as that user, and
  reports the result. stdout goes on the data stream, stderr on
  `SSH_MSG_CHANNEL_EXTENDED_DATA` type 1 (RFC 4254 §5.2) so `ssh host cmd >
  file` does not fold diagnostics into `file`, and the exit is reported as
  `exit-status`, or as `exit-signal` when the child was killed and the signal
  is one of the thirteen the RFC names.
- **The spawn happens before the reply.** §6.5's reply says whether the request
  was *accepted*; a command that could not be started was not, so it gets
  `SSH_MSG_CHANNEL_FAILURE` rather than a SUCCESS followed by an error.
- **Privilege drop, and a refusal instead of a fallback.** sshd binds port 22
  and therefore runs as root. The child is spawned with `CommandExt::gid`/`uid`
  set to the account's, under `#[cfg(unix)]` — which is every target this
  daemon actually runs on, the slateos target spec declaring
  `target-family: ["unix"]`. If the username has **no** `/etc/passwd` entry the
  request is refused, because the only other identity available is the
  daemon's. Running an authenticated user's command as root would have been a
  privilege escalation introduced by the very change that made `exec` work.
- **The environment is built, not inherited.** `env_clear()` then `HOME`,
  `USER`, `LOGNAME`, `SHELL`, `PATH`. Inheriting init's environment leaks the
  daemon's configuration to a remote user and lets a variable set at boot
  change how their commands behave.
- **`pty-req` and `shell` now answer FAILURE.** See the next entry.
- **Output is split to the peer's `max_packet`.** `remote_max_packet` was
  parsed from `CHANNEL_OPEN` and then carried an `#[allow(dead_code)]`;
  exceeding it is a protocol violation and a command whose output is larger
  than one packet is the ordinary case.
- **Inbound channel data is dropped instead of echoed.** With `exec`'s stdin on
  `/dev/null` and `shell` refused, nothing is waiting on the channel's input.
  The receive window is credited back so a client that keeps sending is not
  left blocked on a window that never reopens.

### Verification

123 tests pass (`cargo test -p sshd --target x86_64-pc-windows-gnu`), clippy
clean on both the host and `x86_64-slateos`. New tests cover `/etc/passwd`
parsing including malformed lines and the empty-shell default, the environment
being built solely from the account, the RFC signal-name table, `exit-status`'s
`want_reply = false` framing, and that the chunk overhead constant matches the
real header size.

### What this does *not* mean

`exec` is real but not yet streamed, and `shell` is refused outright. Both are
recorded in the next entry rather than left to be discovered.
