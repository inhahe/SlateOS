## D-POSIX-A-DATAGRAM-UNIX-SOCKET-CANNOT-BE-PASSED — a datagram or sequenced-packet Unix socket cannot be sent through `SCM_RIGHTS` or inherited across `posix_spawn` and `exec` (lane D, 2026-10-16) — OPEN
**Status:** OPEN -- waiting on lane A (`requests/d-a-a-received-unix-socket-needs-its-type.md`).

**In short:** a program can hand an open socket to another: send it
through a Unix socket (`SCM_RIGHTS`), or leave it open for a child it
starts. The C library keeps each socket's type -- stream, datagram or
sequenced-packet -- itself, and decides by it how a send or receive of
several pieces (`iovec`s) behaves: one message, or a run of bytes. A socket
handed over arrives with no type, and the kernel has no call that reports
one. So only a stream socket -- the common case, an accepted connection
handed to a worker -- is handed over, and installed as one. A datagram or
sequenced-packet socket is refused by `sendmsg` (`EOPNOTSUPP`) and is not
open in the child (`EBADF`). Linux hands over both.

Taking it for a stream instead, as the library did on 2026-10-16, broke
its messages silently: a `sendmsg` of a header and a body went as two
packets, and a `recvmsg` into two pieces took one whole packet into each,
cutting the first to the first piece's length without `MSG_TRUNC`
(Opus's review of lane D's round 205).

**Where:** `posix/src/unix_socket.rs` -- `right_of`, which refuses it;
`posix/src/spawn.rs` -- `inheritable_fds`, which skips it;
`land_rights` and `posix/src/crt.rs`'s `retrieve_initial_fds`, which
install what arrives as a stream socket.

**The proper fix:** lane A's `UNIX_OPT_TYPE` for `SYS_UNIX_GET_OPTION`
(the request above): `land_rights` and `retrieve_initial_fds` ask it for
each Unix socket they install, `right_of` and `inheritable_fds` stop
refusing, and `getsockopt(SO_TYPE)` can answer from it.
