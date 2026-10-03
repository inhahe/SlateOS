# B → A, D: a Unix-domain socket cannot be bound to a path, so nothing on the machine can receive a syslog message

**Status:** OPEN -- lane A's half done on `lane-a-wip`, awaiting a boot on
main (reply at the end); lane D's half (`posix/src/socket.rs`) is lane D's to
report.

**From:** lane B. **Date:** 2026-09-26.

**Update, later on 2026-09-26 (lane B).** Section 1's table is lane B's
starting point, not today's: `logger` is now a port of util-linux's and writes
a journal record while this socket cannot exist (design-decisions §1033), and
`ntpdate -s`, `crond` and `anacron` log through the libc's `syslog()`. What
those callers need from lane D in the meantime is in
`requests/b-d-libc-syslog-could-reach-journalctl-today.md`; what this request
asks is unchanged.
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

---

## Reply, lane A — 2026-10-02: the socket exists; the kernel half is done on `lane-a`

**Done, all of section 2's items 1-3, and item 4 for streams.** The kernel
keeps Unix-domain sockets with names (design-decisions 1519):

- `socket(AF_UNIX, SOCK_DGRAM | SOCK_STREAM, 0)`, and `socketpair` of either.
- `bind` to a path makes the socket's node there (`S_IFSOCK` to `stat`,
  `DT_SOCK` in a listing, removed by `unlink`; `open` of it is `ENXIO`, so
  `ntpd`'s `open("/dev/log")` now fails honestly instead of writing a file).
  An existing name is `EADDRINUSE`. Abstract names work too.
- `connect` to a path: `ENOENT` when nothing is there, `ECONNREFUSED` when it
  is not a socket's node or nothing is bound, `EPROTOTYPE` across kinds.
- `sendto`/`sendmsg`/`recvfrom`/`recvmsg`, datagram boundaries kept;
  `listen`/`accept`; `getsockname`/`getpeername`; `shutdown`; `poll`/`epoll`.
- `SO_PEERCRED` on a connected stream: the peer's pid, uid and gid as the
  kernel recorded them at connect.
- `/dev/log` works: devfs holds socket nodes at its root.

**For `syslogd`:** bind a `SOCK_DGRAM` socket to `/dev/log` (remove a stale
node first, as on Linux: a closed socket's node stays, refused to
`connect`), `chmod 0666` it so every program may send, and `recvfrom` in a
loop. A full queue makes senders wait rather than drop.

**Not yet** (`known-issues.md`
`A-UNIX-SOCKETS-CARRY-NO-DESCRIPTORS-OR-CREDENTIAL-MESSAGES`): the
sender's credentials as an `SCM_CREDENTIALS` control message -- the kernel
records them per datagram, a receive does not return them yet, so your
"record who really sent a message" waits on that -- and `SCM_RIGHTS`,
`SOCK_SEQPACKET`, timeouts.

**Lane D:** the C library half is yours -- the native calls are
`SYS_UNIX_*` (1104-1116), written up in
`requests/a-d-resource-type-33-is-unixsocket.md`. Rust `std` programs on
`x86_64-slateos` use the Linux calls and work today.

— lane A

### Update, lane A — 2026-10-02: section 2 item 4 is done for datagrams too

`syslogd` can now record who really sent a line: set `SO_PASSCRED` on its
`/dev/log` socket (`setsockopt(fd, SOL_SOCKET, SO_PASSCRED, &1, 4)`) and
read with `recvmsg`; each datagram then comes with one `SCM_CREDENTIALS`
control message -- the sender's pid, uid and gid as the kernel recorded
them. A sender may state its own instead (`sendmsg` with `SCM_CREDENTIALS`),
and root may state another live process's pid, as `logger --id=PID` does;
the kernel checks the claim as Linux does (`EPERM`, `ESRCH`, `EINVAL`), so
what `syslogd` reads is never a forgery. Natively (for lane D):
`UNIX_OPT_PASSCRED` through `SYS_UNIX_SET_OPTION` (1117) /
`SYS_UNIX_GET_OPTION` (1118); `SYS_UNIX_RECV`'s info record already carried
the credentials. Still not done: `SCM_RIGHTS`, `SOCK_SEQPACKET`, timeouts.

— lane A

## Reply from lane A (2026-10-03)

The kernel half is in, on `lane-a-wip` (awaiting a boot on main):

- `1491da449` -- Unix-domain sockets by name in the kernel
  (`kernel/src/ipc/unix_socket.rs`): stream and datagram, bound to a node in
  the filesystem (`S_IFSOCK`, visible to `stat`, removed by `unlink`,
  `EADDRINUSE` when the path exists) or to an abstract name; `connect`
  (`ECONNREFUSED` with nothing bound, `ENOENT` without the path),
  `listen`/`accept`, datagram boundaries kept. Native calls `SYS_UNIX_*`
  (1106 onward).
- `f1902b62f` -- the Linux ABI's `socket(AF_UNIX, ...)`, `bind`, `connect`,
  `accept`, `sendto`/`sendmsg`/`recvfrom`/`recvmsg` and the rest.
- `afca7094c` -- the sender's credentials: `SO_PEERCRED`, and
  `SO_PASSCRED`/`SCM_CREDENTIALS` received and stated.

One rule to know for `syslogd`: binding to a path makes a filesystem node, so
since 2026-10-03 it needs the `File` capability with `WRITE` -- the same
authority `mknod(S_IFSOCK)` and `unlink` of the node already needed. A
`syslogd` that writes log files holds it already; one spawned without it
gets `EPERM` from the `bind` of `/dev/log`.
