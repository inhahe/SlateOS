## TD-B-SSHD-DROPS-GLOBAL-REQUESTS-SO-SERVERALIVEINTERVAL-KILLS-THE-SESSION (lane B) — FIXED 2026-09-05

**Status:** FIXED — `userspace/sshd/src/main.rs`, commit on `lane-b` 2026-09-05.

**In short:** the option people set to *keep* an ssh session alive was the one
that killed it. `ServerAliveInterval` makes the client poke the server every N
seconds and hang up if the server does not answer; sshd never answered, so the
client concluded the server was dead and closed a session that was perfectly
healthy. Now it answers.

### What was wrong

`dispatch_channel_message` had no arm for `SSH_MSG_GLOBAL_REQUEST` (message type
80, RFC 4254 §4). It fell through to the catch-all, which wrote a line to the
debug log and returned. Nothing was sent back.

RFC 4254 §4 says a global request carrying `want_reply = true` must be answered,
with `SSH_MSG_REQUEST_SUCCESS` or `SSH_MSG_REQUEST_FAILURE`. The reply is the
only signal the client gets, and — critically — *`FAILURE` is a perfectly good
answer*. A client cannot distinguish "the server refused" from "the server is
not there" by waiting, because both look identical from the outside.

OpenSSH's liveness probe is built on exactly that. With `ServerAliveInterval N`
set, the client sends `GLOBAL_REQUEST keepalive@openssh.com` with
`want_reply = true` every N seconds. It picks that name *because* it knows no
server implements it: any reply at all, `FAILURE` included, proves the far end is
alive and still parsing packets. Our silence read as a lost probe. After
`ServerAliveCountMax` of them — default 3 — the client tears the connection down.

| Client configuration | What the user expected | What happened |
|---|---|---|
| `ServerAliveInterval 15` (a common NAT workaround) | session survives an idle period | session killed after ~45 s of idle |
| `ServerAliveInterval 60`, `ServerAliveCountMax 3` | detect a genuinely dead server | every session dies after ~3 min |
| default (no keepalive) | — | unaffected; this is why it went unnoticed |

The failure is worst for the user who did the most right thing: keepalives are
what you turn on when a firewall drops idle flows, so the configuration aimed at
long-lived sessions was the only one that could not have them.

### Two smaller holes found in the same dispatch

- **No `SSH_MSG_UNIMPLEMENTED` for anything.** The constant existed at line 1370
  and was `#[allow(dead_code)]` — it had never been sent. RFC 4253 §11.4 requires
  an unrecognised message to be answered with `UNIMPLEMENTED` carrying the
  sequence number of the packet being rejected.
- **`window-change` ignored `want_reply`.** RFC 4254 §6.7 says `want_reply`
  SHOULD be false for that request, and every client we have met sets it false —
  but SHOULD is not MUST, and a client that set it waited forever. The same arm
  also dropped a payload shorter than 8 bytes with no reply and no log line.

### How it was fixed

| Message | Before | After |
|---|---|---|
| `GLOBAL_REQUEST` (80), `want_reply` | dropped | `REQUEST_FAILURE` (82) |
| `GLOBAL_REQUEST` (80), no reply wanted | dropped | dropped, logged by name |
| unrecognised type | dropped | `UNIMPLEMENTED` (3) + rejected sequence number |
| `DEBUG` (4) | dropped by catch-all | dropped by an explicit arm |
| `UNIMPLEMENTED` (3) | dropped by catch-all | dropped by an explicit arm — see below |
| `KEXINIT` (20) post-auth | dropped by catch-all | dropped by an explicit arm, logged plainly |
| `window-change` with `want_reply` | dropped | `CHANNEL_SUCCESS`, or `CHANNEL_FAILURE` if too short |

`UNIMPLEMENTED` needs its own arm rather than the catch-all, or a peer that sent
`UNIMPLEMENTED` for our `UNIMPLEMENTED` would get another one back and the two
ends would ping-pong until a socket buffer filled.

`KEXINIT` is deliberately **not** answered with `UNIMPLEMENTED`: it is a message
we recognise perfectly well and simply do not support post-handshake, so that
reply would point the client at the wrong problem. See the separate rekey entry
below.

### The shape the fix took, and why

The reply decision is a pure function, `global_request_reply(payload) -> (name,
Option<reply>)`, and the sequence-number arithmetic is a method,
`ConnectionState::current_recv_seq()`. Both were split out from the handlers so
they could be tested without a socket — sshd's unit tests run against a
connection whose handle is `0` and is never written to, so a test that needed a
real send would simply not have been written, and the reply *is* the entire
behaviour here.

`current_recv_seq()` exists because of a genuine off-by-one: both receive paths
advance `recv_seq` the moment a packet is produced, so during dispatch the
counter already names the *next* packet. Sending it raw would have named a packet
the client had not sent yet.

**Verified:** 7 new tests, 191 passing in `cargo test -p sshd`, clippy clean.
The keepalive test asserts the exact bytes an OpenSSH probe gets back.
