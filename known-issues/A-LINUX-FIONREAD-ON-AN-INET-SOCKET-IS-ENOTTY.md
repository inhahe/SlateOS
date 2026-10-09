### [A] `ioctl(FIONREAD)` on an `AF_INET` socket answers `ENOTTY` -- 2026-10-09

**Status:** OPEN -- fixed on lane-a-wip 2026-10-09, awaiting a boot on main.

**In short:** a Linux program can ask any descriptor how many bytes are
waiting to be read (`ioctl(fd, FIONREAD, &n)`). Since 2026-10-09 the kernel
answers for files, pipes, the terminal and Unix-domain sockets, as Linux
does. For an Internet socket (TCP or UDP, through the netstack daemon) it
answered "not supported" (`ENOTTY`), so a program that sizes its read
buffer that way -- GLib's `g_socket_get_available_bytes`, some event loops,
`nc` -- got an error where Linux gives a count.

**Where:** `kernel/src/syscall/linux.rs`, `fionread_count`: there was no
`HandleKind::Socket` arm, so it fell to `ENOTTY`.

**Why it was not there at first:** the bytes of a kernel-side `net::socket`
are held by the userspace netstack daemon, not the kernel. The kernel learned
readiness by asking it (`NetstackConn::poll_on`, `OP_POLL`), which said
*whether* bytes wait, not how many.

**The fix (lane-a-wip, 2026-10-09):** the daemon puts the count in the
`OP_POLL` completion's spare `flags` word and says so with a new result bit,
`netipc::ring::POLL_COUNTED`: a TCP connection's buffered in-order bytes
(`rx_len`), a UDP socket's next datagram's payload (`UdpSock::next_len`), 0
for a listener or a handshake in flight. The kernel reads it
(`NetstackConn::readable_bytes_on`, `net::socket::readable_bytes`) and
answers as Linux's `tcp_ioctl`/`udp_ioctl` do: the count, 0 for a stream not
connected or an unbound datagram socket, `EINVAL` for a listener. Checked by
`net::socket::self_test_fionread` on a loopback connection, and the wire
format by netipc's `a_poll_completion_carries_its_count_in_flags`.
