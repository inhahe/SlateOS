# B → A, D: a Unix-domain socket cannot be bound to a path, so nothing on the machine can receive a syslog message

**Status:** OPEN. Nothing is broken by it today that a user would notice at
once — every writer has a stopgap — but each stopgap loses or misfiles
messages, and lane B's side of the fix (a `syslogd` that listens, a `logger`
that is util-linux's) waits on this.

**From:** lane B. **Date:** 2026-09-26.
**Touches:** `posix/src/socket.rs` (`socket`, `bind`, `connect`, `sendto`,
`recvfrom`, lane D) and whatever kernel object backs a socket that has a name
in the filesystem (lane A).

## In short

On SlateOS, `socket(AF_UNIX, ...)` fails with `EAFNOSUPPORT`: the only
Unix-domain sockets that exist are unnamed stream pairs from `socketpair`.
There is no way to create a socket at a path such as `/dev/log` and no way to
connect to one.

What a user sees: system log messages that go to four different places or
nowhere, depending on which program wrote them — and `journalctl`, which
understands only one of those formats, shows almost none of them. What is being
asked: path-bound `AF_UNIX` sockets — `SOCK_DGRAM` first, since that is what
syslog uses, and `SOCK_STREAM` beside it, since most other users of
Unix-domain sockets want that.

## 1. What each writer does today, read from the code

| writer | where its messages go | lane |
|---|---|---|
| libc `syslog()` | stderr — `posix/src/syslog.rs`: "Our OS doesn't have a syslog daemon" | D |
| `logger` | appended as text to `/var/log/syslog`, or to stdout if that fails | B |
| `ntpdate -s` (`userspace/ntpd`) | `open("/dev/log")` as a *file*; the error is discarded, so the message is lost | B |
| `syslogd log ...` | a JSON-lines record in `/var/log/syslog.jsonl` | B |
| `syslogd daemon` | receives nothing: "for now, the daemon sits idle" | B |
| `journalctl` | reads `/var/log/syslog.jsonl` and `/var/log/syslog`, but parses only the JSON-lines records, so `logger`'s text lines never appear | B |

Every POSIX program that logs does it the libc way — connect a datagram
socket to `/dev/log` and send one frame per message — and so does util-linux's
`logger`, which lane B wants to port faithfully (it would then need somewhere
to send). None of that can work while the socket cannot exist.

## 2. What is asked

1. `socket(AF_UNIX, SOCK_DGRAM, 0)` and `socket(AF_UNIX, SOCK_STREAM, 0)`.
2. `bind` to a filesystem path, creating the socket's node there (`S_IFSOCK`,
   visible to `stat`, removed by `unlink`), with `EADDRINUSE` when the path
   exists; `connect` to such a path (`ECONNREFUSED` when nothing is bound,
   `ENOENT` when the path does not exist).
3. `sendto`/`sendmsg`/`recvfrom`/`recvmsg` with datagram boundaries kept, and
   `listen`/`accept` for the stream type.
4. The sender's credentials, if the kernel can supply them (`SO_PEERCRED`,
   `SCM_CREDENTIALS`): a syslog daemon uses them to record who really sent a
   message, rather than trusting the PID written in it. Useful, not required
   for a first version.

The `/dev/log` consumer is lane B's: `syslogd` binds it and writes each
message as a journal record, `logger` sends to it as util-linux does, and
`ntpd` stops opening it as a file. Lane D's libc `syslog()` can then send
there too, as glibc does, instead of to stderr — that one is lane D's own
call.

## 3. Why not a native IPC channel instead

SlateOS's own services should log over channel IPC, and nothing here argues
otherwise. But `/dev/log` is where every ported program's libc sends, and it
is a path, not a service name. Either the POSIX layer emulates it on top of a
channel, or the socket exists; both need the answer from lanes A and D, and
path-bound Unix sockets are wanted well beyond syslog — X11, D-Bus,
PostgreSQL's local socket, `ssh-agent`, `tmux` and `screen` all use them.
