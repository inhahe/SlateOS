## D-POSIX-A-RECEIVED-UNIX-SOCKET-IS-TAKEN-FOR-A-STREAM — a Unix socket that arrives through `SCM_RIGHTS` is driven as a stream socket whatever its type (lane D, 2026-10-16) — OPEN
**Status:** OPEN -- waiting on lane A (`requests/d-a-a-received-unix-socket-needs-its-type.md`).

**In short:** a program can hand an open socket to another through a Unix
socket (`SCM_RIGHTS`). The C library keeps each socket's type -- stream,
datagram or sequenced-packet -- itself, and decides by it how receives and
sends behave: whether a receive may take part of a message, whether a send
of several pieces is one message. A socket that arrives this way comes
with no type, and the kernel has no call that reports one, so the library
installs it as a stream socket. A received datagram or sequenced-packet
socket then reads like a stream: a receive into a short buffer keeps the
rest of the message for the next receive instead of discarding it, and
`MSG_TRUNC` is not reported. A stream socket -- the common case, an
accepted connection handed to a worker -- is right.

The same holds for one a child inherits across `posix_spawn` or `exec`:
its crt0 learns the handle and nothing of the type.

**Where:** `posix/src/unix_socket.rs` -- `land_rights`, which installs
what `SYS_UNIX_RECVMSG` hands back; `posix/src/crt.rs` --
`retrieve_initial_fds`, which installs what a child inherits.

**The proper fix:** lane A's `UNIX_OPT_TYPE` for `SYS_UNIX_GET_OPTION`
(the request above); `land_rights` and `retrieve_initial_fds` ask it
for each Unix socket they install, and `getsockopt(SO_TYPE)` can answer
from it.
