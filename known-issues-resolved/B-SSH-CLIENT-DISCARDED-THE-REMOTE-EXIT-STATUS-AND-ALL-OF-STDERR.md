## B-SSH-CLIENT-DISCARDED-THE-REMOTE-EXIT-STATUS-AND-ALL-OF-STDERR — 2026-08-21 — FIXED

**In short:** our `ssh` client exited 0 no matter what the remote command
returned, and threw away every byte the remote command wrote to stderr without
so much as a warning. So `ssh host false` looked like a success, and
`ssh host 'cc broken.c'` printed nothing at all about why the compile failed.
Both now work: the client exits with the remote status, and remote stderr comes
out on the local stderr.

**Where it was:** `userspace/ssh/src/main.rs` — `handle_channel_request`,
`process_server_message`, and `main`.

### What was wrong

Three separate holes, all in the same direction — information the server sent
that the client silently dropped.

**1. `exit-status` was parsed and discarded.** `handle_channel_request` read
the request type, logged it at `-v`, and returned. `main` then exited 0
unconditionally. This is the mirror of the sshd bug fixed the same day
(`B-SSHD-EXEC-REPLIED-WITH-THE-COMMAND-INSTEAD-OF-RUNNING-IT`): between the
two, a failing remote command was reported as a success at *both* ends, and
fixing only the server would have left the client still lying.

**2. `SSH_MSG_CHANNEL_EXTENDED_DATA` was not handled at all.** Message type 95
was not even in the client's `msg` module, so it fell through
`process_server_message`'s catch-all to `verbose("unhandled message type: 95")`
— invisible unless `-v` was passed. Everything a remote command wrote to
stderr vanished.

**3. A latent MAC desynchronisation.** When a server-initiated request set
`want_reply`, the client replied by calling `tcp_send_all` directly with
`build_packet(..., self.seq_send, ...)` and **did not advance `seq_send`**. The
sequence number is an input to every packet's MAC, so the next packet the
client sent would have been MACed with a number the server had already moved
past, and the server would have rejected it and everything after it. The
function took `&self`, which is why it could not advance the counter — the
signature was the bug. It is `&mut self` now and goes through `send_packet`.

Never observed in practice only because no server this client talks to sends a
channel request with `want_reply` set. `exit-status` and `exit-signal` must
both have it clear (RFC 4254 §6.10).

### The fix

- `exit-status` and `exit-signal` are parsed into a `RemoteExit` on the
  session, and `main` exits with it.
- Exit codes now follow `ssh(1)` exactly: the remote command's status on
  success, **255** for any failure of the client or the connection. The four
  `process::exit(1)` sites became `EXIT_SSH_FAILURE`. Reserving one code is
  what lets a caller distinguish "the command failed" from "the command never
  ran"; with 1 doing double duty it could not.
- A command killed by a signal has no status of its own; it is reported on
  stderr and exits 255, matching OpenSSH.
- Extended data type 1 goes to stderr; any other type code is discarded with a
  verbose note rather than being written to stdout, where it would corrupt the
  command's output. Malformed messages are dropped whole.
- `shell request failed` / `exec request failed` are now distinguished in the
  error text, and name the channel. They have different causes — a server with
  no pseudo-terminal support refuses `shell` and accepts `exec`, which is
  exactly what SlateOS's own sshd now does — and a combined message sends the
  reader looking in the wrong place.

### A note on what could not be fixed

A server that sends **no** `exit-status` still yields exit 0, because that is
what an interactive session looks like and the client cannot tell the two
apart. This is not a client-side gap that can be closed; it is the reason a
*server* must always send one, and is why the sshd fix was the necessary half.

### Verification

The client had **no test module at all** — 2556 lines, zero tests. It has 11
now, covering: a status recorded and returned; a non-zero status not becoming
success; a missing status meaning success; `exit-signal` including that the
name is stored verbatim without a `SIG` prefix; an unknown request not
overwriting a status already received; a truncated `exit-status` being an error
rather than a silent zero; unknown and malformed extended data being dropped;
and that `EXIT_SSH_FAILURE` is 255. Clippy clean on the host and on
`x86_64-slateos`.
