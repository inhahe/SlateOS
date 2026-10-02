### A-SOCKETS-HAVE-NO-PENDING-ERROR -- 2026-10-02 -- OPEN (lane A)

**Status:** OPEN (lane A) -- a small difference from Linux, found while
making `sendmmsg`/`recvmmsg` real.

**In short:** when `recvmmsg` has received some messages and the next one
fails (a bad pointer in a later entry, say), it answers with the count, as
Linux does -- but Linux also keeps the failure on the socket, so the next
call on it (or `getsockopt(SO_ERROR)`) reports it, and here it is dropped.
A program sees the same failure again only if its cause is still there.
Nothing ported is known to depend on it.

**Where:** `kernel/src/syscall/linux.rs` `sys_recvmmsg` (the run's end);
`kernel/src/ipc/unix_socket.rs` and `kernel/src/net/socket.rs` keep no
pending error per socket.

**Proper fix:** a pending-error slot on each socket object (Linux's
`sk_err`), set by `recvmmsg` for a failure other than `EAGAIN` after a
partial run, taken and cleared by the next receive and by `SO_ERROR` -- the
same slot an asynchronous error (a refused datagram, a reset connection)
would use.
