## 1186. Unix sockets with names: stream pairs stay on the old pair until spawn can carry the new, `FIONREAD` peeks, and the kernel's timeouts are remembered for the restart rule

**Date:** 2026-10-07
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** the kernel gained Unix-domain sockets that have names (lane
A's `SYS_UNIX_*`, `requests/a-d-resource-type-33-is-unixsocket.md`), and the C library now builds
`socket(AF_UNIX, ...)` on them (`posix/src/unix_socket.rs`). Before that it
had only `socketpair(AF_UNIX, SOCK_STREAM)`, on an older kernel object that
cannot have a name. Three choices about how the two meet. Stream pairs stay
on the old object for now, because only it can be handed to a program
started with `posix_spawn` or `exec`. `ioctl(FIONREAD)` -- how much is
waiting -- is answered by looking at the data without taking it, because the
kernel has no count yet. And each socket's receive and send timeouts are
remembered by the library as well as the kernel, so that a signal can be
handled the way Linux handles it without asking the kernel first.

### 1. Which kind a stream `socketpair` makes

| Option | *What changes* | For | Against |
|---|---|---|---|
| **A. The old socket pair, until spawn can carry the new** (chosen) | stream pairs cross `posix_spawn`/`exec` as before | nothing a parent hands a child today is lost -- the usual way a parent talks to a child | two kinds of Unix socket in the library for a while; a stream pair has no `SO_PASSCRED` or `SO_PEERCRED` |
| B. The new kind for every pair | one kind; credentials on stream pairs too | simpler | a child started with a stream pair end would silently lose it: a regression for every program that does this |

Datagram and sequenced-packet pairs, which the old object never had, are the
new kind from the start. The day lane A gives the new kind a spawn handle
type (`requests/d-a-a-named-unix-socket-must-cross-spawn-and-count-its-bytes.md`),
stream pairs move too and the old kind can go.

### 2. `FIONREAD`

| Option | *What changes* | For | Against |
|---|---|---|---|
| **A. Peek** (chosen): a non-blocking receive with `UNIX_PEEK` into a 64 KiB buffer, whose length is the answer | programs that size their reads by it work | right for a datagram (the next message's length) and for a stream up to one call's worth | a copy where a count would do, and a 64 KiB allocation per call |
| B. `EINVAL` until the kernel counts | nothing is copied | honest | a program that asks gets an error Linux never gives |

### 3. Timeouts and signals

Linux restarts a socket wait that a signal interrupted when the handler has
`SA_RESTART` -- unless the socket has a timeout for the call, when it answers
`EINTR` (signal(7)). The rule is the library's to apply
(`crate::interrupt`), and the timeout is the kernel's. The library writes the
timeout to the kernel and keeps a copy in the socket's metadata, which the
rule reads. An accepted connection copies its listener's, as the kernel's
does. The copy can go stale if another process sharing the socket (after
`fork`) changes it; then a wait may be restarted that Linux would not have,
or not restarted when it would. The alternative, asking the kernel before
every blocking call, costs a system call on every receive.

**Where it bites:** `posix/src/unix_socket.rs`, `posix/src/socket.rs`
(`socketpair`, `set_unix_timeout`), `posix/src/ioctl.rs` (`FIONREAD`),
`posix/src/spawn.rs` (`inheritable_fds`).
