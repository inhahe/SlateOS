### A-UNIX-SOCKETS-CARRY-NO-DESCRIPTORS-OR-CREDENTIAL-MESSAGES -- 2026-10-02 -- FIXED (lane A)

**Status:** FIXED (lane A), 2026-10-02 -- everything the first version of
named Unix-domain sockets (design-decisions 1519) left out is done. What
remains of descriptor passing has entries of its own.

**In short:** programs can meet at a socket's name, talk, pass open files,
state their credentials, and use all three kinds -- stream, datagram and
sequenced packets.

Done the same day:
- **`SOCK_SEQPACKET`**: connected like a stream (`listen`, `connect`,
  `accept`, `socketpair`), each send arriving whole like a datagram, cut to
  the buffer with `MSG_TRUNC`; what a client sends before `accept` waits in
  the server's socket, made at `connect` as Linux makes it; the peer's close
  is end of file after its messages, and a send to it `EPIPE`. `SOCK_RAW`
  makes a datagram socket, as Linux's `unix_create` does.
- **Passing open files** (`SCM_RIGHTS`, design-decisions 1521): on a datagram,
  and on a stream riding on the bytes their send wrote; close-on-exec on
  request; `MSG_CTRUNC` with the surplus released; a per-user limit on
  descriptors in flight (`ETOOMANYREFS`); sockets kept alive only by each
  other collected as garbage. What is left of it: A-SCM-RIGHTS-DIFFERENCES,
  A-NATIVE-PROGRAMS-CANNOT-PASS-DESCRIPTORS.
- **Receive and send timeouts** (`SO_RCVTIMEO`/`SO_SNDTIMEO`): a blocking call
  waits at most the limit and then answers `EAGAIN`, a signal during such a
  wait answers `EINTR`, and an accepted socket starts with its listener's
  limits, as on Linux.
- **Credentials as a control message** (`SCM_CREDENTIALS` with
  `SO_PASSCRED`): `recvmsg` hands back the sender's credentials, and a sender
  may state its own (or, as root, another live process's) on `sendmsg`,
  checked as Linux checks them.

**Where:** `kernel/src/syscall/linux.rs` `sys_socket`, `sys_socketpair`;
`kernel/src/ipc/unix_socket.rs`.
