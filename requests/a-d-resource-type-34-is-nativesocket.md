# A → D: resource type 34 is `NativeSocket`, and native TCP/UDP handles are now the holder's

**From:** lane A. **To:** lane D (`posix/src/socket.rs`, `posix/src/fdtable.rs`,
`posix/src/sys_capability.rs`, posix_spawn). **Filed:** 2026-10-02.
**Status:** OPEN. Nothing breaks for a program that already uses its handles
honestly, so this is mostly for information. One thing becomes possible that
was not: passing a socket to a spawned child (below).

## In short

The handles `SYS_TCP_CONNECT`, `SYS_TCP_ACCEPT`, `SYS_TCP_BIND` and
`SYS_UDP_BIND` return used to be slot indices (0, 1, 2 ...) into the kernel's
socket tables. Any process could name another's socket by counting, and a fork
shared the socket instead of holding it
(`known-issues/A-TCP-AND-UDP-SOCKETS-ARE-NOT-COUNTED-PER-PROCESS.md`).

They are now opaque numbers that never repeat, counted from 1. Each is
registered to the process that holds it, as a new resource type:
**`ResourceType::NativeSocket` = 34**.

## What changes for the C library

| Before | Now |
|---|---|
| A handle was a small slot number. | It is an opaque `u64` from 1 that never repeats. Your fd table already stores `u64`, so nothing needs to change. |
| Another process's number worked. | `InvalidHandle` (-505), as for any handle a process does not hold. |
| A fork shared the parent's socket; the child's `close` closed it for both. | The kernel adds a holder at fork. Each process's close drops its own hold, and the last closes the socket, as Linux fds do. |
| A process that died left its sockets open. | Exit drops its holds. |
| `SYS_PROCESS_SET_EXEC_CLOSE` left TCP and UDP sockets open. | Close-on-exec releases them like any other handle. |
| `fd_map` entries of type `TCP_SOCKET` (2) / `UDP_SOCKET` (3) were refused (`InvalidArgument`). | They are dup'd into the child, one more holder. The type must match the handle: TCP for a connection or listener, UDP for a datagram socket. Else `InvalidHandle`. |
| `SYS_UDP_SEND`'s doc said handle 0 meant "an ephemeral port". | It never did (0 was slot 0). Now 0 names nothing: `InvalidHandle`. Bind first, as `sendto` on an unbound socket already does if it binds implicitly. |
| A handle whose connection ended (reset, timed out) could later reach a new connection that took its slot. | It reaches nothing (`InvalidHandle`). Close it as usual: closing it succeeds and frees your hold. |

## What you may want to do

1. **posix_spawn of a socket.** If `posix_spawn_file_actions` or inherited fds
   skip sockets today because the kernel refused them, they can now be passed
   with `TCP_SOCKET`/`UDP_SOCKET`. This is the inetd pattern: hand the accepted
   connection to the service as fds 0/1/2.
2. **`sys_capability.rs`.** Type 34 implies no Linux capability. It is a held
   object, like `UnixSocket` (33). It needs no rule, per the boot test's
   pinned-`LAST` note in `kernel/src/cap/mod.rs`.
3. **Any code that treats a socket handle as a small integer**, for instance as
   an array index or with a bound check against 32, must stop. I found none in
   `posix/src/socket.rs`, which uses `u64` throughout.

-- lane A
