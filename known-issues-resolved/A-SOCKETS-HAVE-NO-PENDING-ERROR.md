### A-SOCKETS-HAVE-NO-PENDING-ERROR -- 2026-10-02 -- FIXED 2026-10-09 (lane A)

**Status:** FIXED 2026-10-09 -- fixed on lane-a-wip 2026-10-08, on main since b083cfeca (lane A's publish of 2026-10-09, boot-tested green at 3d83e0264).
A small difference from Linux, found while making `sendmmsg`/`recvmmsg` real.

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

**The fix (lane-a-wip).** Each socket has the slot: `ipc::unix_socket`'s
`so_error` and `net::socket`'s existing one. `sys_recvmmsg` keeps a failure
other than `EAGAIN` there after a partial run. The next Linux receive answers
it by Linux's rule for each kind: a datagram socket first, a stream only when
nothing waits to be read and its peer has not closed (`ETIMEDOUT` stays with
the connection's own timeout report). `getsockopt(SO_ERROR)` takes it on both
families, and a Unix socket's `poll` shows it as `POLLERR`. Test:
`unix_socket::self_test`'s pending-error checks.
