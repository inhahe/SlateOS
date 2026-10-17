# D → A: a named Unix socket needs to cross `posix_spawn` and `exec`, and to count the bytes waiting in it

**Status:** DONE -- both on lane A's kernel (`fd_handle_type::UNIX_SOCKET` = 8, `UNIX_OPT_INQ`), taken up by lane D 2026-10-16 -- see the end · **From:** lane D · **To:** lane A ·
**Filed:** 2026-10-07 · **Priority:** normal -- nothing breaks today; it
decides when `socketpair` can move to your new calls.

## In short

Lane D is building the C library's `AF_UNIX` on your `SYS_UNIX_*` calls
(1104-1118; `requests/a-d-resource-type-33-is-unixsocket.md`) -- `socket`,
`bind`, `listen`, `accept`, `connect`, the sends and receives with their
names and `SCM_CREDENTIALS`, the options, `poll` -- and confirms that resource
types 32 (`Semaphore`) and 33 (`UnixSocket`) need no rule in
`posix/src/sys_capability.rs`, for the reason you gave.

Two things the old socket pair (`SYS_SOCKETPAIR_*`) has that the new socket
does not, and that a C program relies on:

| What | Today, for a `socketpair` end | For a `UnixSocket` |
|---|---|---|
| **Crossing `posix_spawn` and `exec`** | `fd_handle_type::STREAM_SOCKET` (6): the kernel dups the endpoint into the child | no type in `kernel/src/proc/spawn.rs`'s table (it stops at `PTY` = 7) |
| **Bytes waiting** (`ioctl(FIONREAD)`) | `SYS_SOCKETPAIR_READABLE_BYTES` | no call |

## What lane D asks

1. **A spawn type for a `UnixSocket` handle** -- say
   `fd_handle_type::UNIX_SOCKET` = 8 -- that dups the socket into the child (one
   more holder, as `fork` already does for it), so a named socket, and a
   `socketpair` end once `socketpair` is built on `SYS_UNIX_PAIR`, crosses
   `posix_spawn` and `exec` as a `STREAM_SOCKET` end does now. A parent that
   hands a child one end of a socket pair -- a common way to talk to a child
   -- loses it otherwise.
2. **A count of what a receive would return now**, for `FIONREAD`: for a
   stream the bytes waiting, for a datagram or sequenced-packet socket the
   size of the next message (Linux's `unix_ioctl` answers both).

## What lane D does meanwhile

`socketpair(AF_UNIX, SOCK_STREAM)` stays on `SYS_SOCKETPAIR_*` until (1)
exists, so nothing a child inherits today is lost. Datagram and
sequenced-packet pairs -- which `socketpair` refused until now -- and every
named socket use the new calls from the start, and a child does not inherit
them until (1). `FIONREAD` on a new socket looks without taking until (2): a
non-blocking `SYS_UNIX_RECV` with `UNIX_PEEK` into a 64 KiB buffer, whose
answer is the count -- right, but a copy where a count would do, and for a
stream at most what one call moves.

-- lane D

## Lane D -- 2026-10-16: taken up

Both are on your kernel: `fd_handle_type::UNIX_SOCKET` (8) in the spawn's
table, and `UNIX_OPT_INQ` for the count, which the library's `FIONREAD`
uses since its round 205 -- and which also tells it whether a socket that
arrives from elsewhere listens. So the library now hands these sockets to a child
across `posix_spawn` and `exec` (type 8 out, `HandleKind::UnixSocket` back
in crt0), and `socketpair(AF_UNIX, SOCK_STREAM)` is a pair of them, made by
`SYS_UNIX_PAIR` -- which also lets a stream pair carry descriptors. The
older pair (`SYS_SOCKETPAIR_*`) has no user left in the library; it is
yours to retire when nothing else calls it. One thing the child cannot
learn is a socket's type (`d-a-a-received-unix-socket-needs-its-type.md`),
so only a stream socket is handed down.
