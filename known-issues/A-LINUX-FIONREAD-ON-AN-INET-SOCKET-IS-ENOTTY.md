### [A] `ioctl(FIONREAD)` on an `AF_INET` socket answers `ENOTTY` -- 2026-10-09

**Status:** OPEN

**In short:** a Linux program can ask any descriptor how many bytes are
waiting to be read (`ioctl(fd, FIONREAD, &n)`). Since 2026-10-09 the kernel
answers for files, pipes, the terminal and Unix-domain sockets, as Linux
does. For an Internet socket (TCP or UDP, through the netstack daemon) it
still answers "not supported" (`ENOTTY`), so a program that sizes its read
buffer that way -- GLib's `g_socket_get_available_bytes`, some event loops,
`nc` -- gets an error where Linux gives a count.

**Where:** `kernel/src/syscall/linux.rs`, `fionread_count`: the
`HandleKind::Socket` arm is missing, so it falls to `ENOTTY`.

**Why it was left:** the bytes of a kernel-side `net::socket` are held by
the userspace netstack daemon, not the kernel. The kernel learns
readiness by asking it (`NetstackConn::poll_on`, OP_POLL), which says
*whether* bytes wait, not how many. A true count needs either a new daemon
op (OP_INQ: the receive queue's length for a TCP connection, the next
datagram's size for UDP -- Linux's `SIOCINQ` answers those two) or a
count carried back on the poll reply. Both are protocol changes on the
`netipc` wire, which the daemon (`services/netstack`, lane A) and the
kernel client must make together; neither is a guess the kernel can make
on its own, and a made-up number would be worse than the error.

**The fix:** add the count to the poll reply (one more field, no new
round trip) and answer `HandleKind::Socket` from it: a connected TCP
socket's receive-queue length, a UDP socket's next datagram size, and
`EINVAL` for a listener, as Linux's `tcp_ioctl` and `udp_ioctl` answer.
Extend `self_test_ioctl_fionread` with a loopback TCP pair.
