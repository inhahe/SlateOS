## D-POSIX-NAMED-UNIX-SOCKETS-DO-NOT-CROSS-SPAWN-OR-EXEC — a Unix socket made by `socket(AF_UNIX, ...)`, `accept`, or a datagram or sequenced-packet `socketpair` is not inherited by a program started with `posix_spawn` or `exec` (lane D, 2026-10-07)
**Status:** OPEN -- waits on lane A's spawn handle type
(`requests/d-a-a-named-unix-socket-must-cross-spawn-and-count-its-bytes.md`)

**In short:** since 2026-10-07 the C library has Unix-domain sockets with
names (`posix/src/unix_socket.rs`), on the kernel's `SYS_UNIX_*` calls: bind
to a path or an abstract name, listen, accept, connect, datagrams, sequenced
packets, credentials. One thing a Linux program can do with them it cannot do
here yet: hand one to a program it starts. The kernel's `posix_spawn` and
`exec` carry a parent's descriptors by type (`fd_handle_type`), and has no
type for these sockets, so a child does not get them: the descriptor is
simply absent in the child, and across `exec` the socket stays held by the
process with no descriptor left to reach it until it exits.

**What still works.** `socketpair(AF_UNIX, SOCK_STREAM)` -- the usual way a
parent talks to a child -- stays on the older socket pair, which does cross
(`fd_handle_type::STREAM_SOCKET`), until the kernel can carry the new kind.
`fork` gives a child every socket, old and new. Servers that a client
connects to by name (syslog, D-Bus, a display server) need no inheritance.

**What does not, until lane A's change.** A parent that hands a child a named
socket, an accepted connection, or one end of a datagram or sequenced-packet
pair: a `posix_spawn` file action naming one fails (`EBADF`), and an `exec`
loses it. Systemd-style socket activation is the common case.

**The proper fix** is lane A's: an `fd_handle_type` for a `UnixSocket`
handle, which dups it into the child as `fork` does. Then lane D maps the kind
in `posix/src/spawn.rs` (`kind_to_handle_type`, `handle_type_to_kind_for`,
`inheritable_fds`), and moves `socketpair(AF_UNIX, SOCK_STREAM)` onto
`SYS_UNIX_PAIR`, so one kind serves every Unix socket.

**Smaller differences from Linux, by the same design** (the module's note
lists them): `getsockname` and the sender's address report the resolved
absolute path where Linux reports the name as it was given; a resolved path
must fit `sun_path`'s 108 bytes; a datagram socket cannot be disconnected by
connecting it to `AF_UNSPEC` (`EOPNOTSUPP`); descriptors cannot be passed
(`SCM_RIGHTS`, `EOPNOTSUPP`), nor credentials other than one's own stated.

**Where:** `posix/src/unix_socket.rs`, `posix/src/spawn.rs`,
`posix/src/socket.rs` (`socketpair`).
