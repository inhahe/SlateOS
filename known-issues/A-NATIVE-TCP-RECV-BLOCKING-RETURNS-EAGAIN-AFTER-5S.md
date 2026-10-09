### [A] A-NATIVE-TCP-RECV-BLOCKING-RETURNS-EAGAIN-AFTER-5S: a blocking native `SYS_TCP_RECV` gives up after five seconds with "try again" -- 2026-09-26
**Status:** OPEN -- fixed on lane-a-wip 2026-10-09, awaiting a boot on main. Found while bounding `sys_tcp_send`'s copy
(`A-USER-SIZED-KERNEL-BUFFERS-NOW-REACH-VMALLOC`, class 2c).

**In short:** there are two TCP implementations. Programs using the ordinary
socket calls reach the netstack daemon. The kernel's own older stack is still
reachable through the native `SYS_TCP_*` calls. On that older path, a program
waiting for data with a *blocking* receive is told "try again" if nothing
arrives within five seconds, instead of being kept waiting. That is the same
family of fault as `A-BLOCKING-TCP-RECV-REPORTS-EOF-AFTER-2S` on the daemon
path, milder: it does not claim the connection closed.

**Where.** `kernel/src/syscall/handlers.rs` `sys_tcp_recv`: the blocking arm
calls `net::tcp::read_blocking(handle, 500, buf_cap)`, which polls for about
5 s. When that returns nothing on an open connection, the handler answers
`WouldBlock`. Its comment says this is so "the POSIX layer can retry or
propagate EAGAIN", and an earlier fix changed the answer from a spurious EOF
to that. A blocking call should not return `EAGAIN` at all.

**Why not fixed with class 2c.** It needs two things established first:
- who still calls the native TCP API, now that sockets route to the daemon
  when `netstack_client::userspace_enabled()`;
- what lane D's POSIX layer does with the `WouldBlock`: if it retries, the
  proper fix -- the kernel waiting until data, EOF or a signal, as
  `net::socket::wait_until` does for the daemon path -- changes nothing a
  caller sees except that the retry stops being needed.

**Proper fix.** Loop in `sys_tcp_recv` for a blocking caller: read, and while
nothing arrives on an open connection, wait -- signal-aware, as
`wait_until` does -- rather than answering `WouldBlock`.

**Fix (lane-a-wip, 2026-10-09).** `net::tcp::wait_readable` waits for as long
as it takes until a read has something to answer -- data, the peer's FIN,
the read side shut, or a reset -- driving the stack (`net::poll`) and
sleeping between looks (1 ms doubling to 10 ms, the daemon path's backoff),
and answers `Interrupted` (EINTR) on a deliverable signal. A blocking
`SYS_TCP_RECV` calls it and then reads; so does a blocking `MSG_PEEK`, which
did not wait at all before (Linux's does). An empty read after the wait is
end of file only at the end of the stream: a reader that finds the data
taken by another reader of the socket waits again rather than reporting
EOF. A zero-length receive answers 0 at once. `read_blocking`, with its
fixed time, stays for the kernel's own clients (HTTP, the web server),
which want one.

Whoever relies on the old `WouldBlock` -- the open question above -- sees
only that a retry is no longer needed: a blocking receive now answers data,
0, an error, or EINTR. Test: `net::tcp`'s `test_wait_readable` (at once for
each answer; waits, here 30 ms, for data another task queues; a handle past
the table refused).
