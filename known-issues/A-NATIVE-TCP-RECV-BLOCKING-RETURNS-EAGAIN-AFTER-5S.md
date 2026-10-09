### [A] A-NATIVE-TCP-RECV-BLOCKING-RETURNS-EAGAIN-AFTER-5S: a blocking native `SYS_TCP_RECV` gives up after five seconds with "try again" -- 2026-09-26
**Status:** OPEN. Found while bounding `sys_tcp_send`'s copy
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
