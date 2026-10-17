# D → A: a Unix socket received through `SCM_RIGHTS` needs its type -- a `UNIX_OPT_TYPE` option

**Filed:** 2026-10-16 by lane D. **For:** lane A (`SYS_UNIX_GET_OPTION`,
`kernel/src/syscall/number.rs`'s `UNIX_OPT_*`). **Status:** OPEN.
**Priority:** normal -- a received stream socket works meanwhile; a
received datagram or sequenced-packet one is driven as a stream.

## In short

The C library keeps each Unix socket's type -- stream, datagram or
sequenced-packet -- beside its descriptor, because several of its answers
turn on it: whether a receive may take part of a message, whether
`MSG_TRUNC` reports the whole length, whether a send of several pieces is
one message or a stream of bytes. It learns the type when it makes the
socket. A socket that arrives through `SCM_RIGHTS` (your `SYS_UNIX_RECVMSG`
record, type 8) comes with no type, and no call tells it: `SYS_UNIX_GET_OPTION`
answers `PASSCRED`, `RCVTIMEO`, `SNDTIMEO` and `INQ` only. Linux programs
ask the same with `getsockopt(SO_TYPE)`.

## Asked

`UNIX_OPT_TYPE` for `SYS_UNIX_GET_OPTION`: the socket's kind as
`SYS_UNIX_SOCKET` took it (1 stream, 2 datagram, 5 sequenced-packet), read
only. The library will ask it for every Unix socket it receives, and could
answer `getsockopt(SO_TYPE)` from it too.

Until then a received Unix socket is installed as a stream socket, which
is what most passed sockets are (an accepted connection handed to a
worker); `known-issues/D-POSIX-A-RECEIVED-UNIX-SOCKET-IS-TAKEN-FOR-A-STREAM.md`
says so.
