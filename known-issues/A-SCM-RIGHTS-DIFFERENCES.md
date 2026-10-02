### A-SCM-RIGHTS-DIFFERENCES -- 2026-10-02 -- OPEN (lane A)

**Status:** OPEN (lane A) -- small differences from Linux left in descriptor
passing (design-decisions 1521). None is known to matter to a ported program.

**In short:** passing open files over a Unix-domain socket works as on
Linux, with four differences a program would have to go looking for:

1. **A socket kept alive only from inside something else.** The collector
   finds sockets that only Unix-socket queues hold. One held only by a
   message in a SlateOS channel (capability transfer) is never collected, so
   a cycle through a channel leaks its sockets until reboot. Linux's
   collector has the same blind spot for its io_uring, which it
   special-cases.
2. **Descriptors installed but never named.** A receive checks that the
   control buffer is writable, installs the descriptors, then writes their
   numbers. If another thread unmaps the buffer in between, the call fails
   with `EFAULT` and the descriptors stay installed under numbers the program
   was never told. Linux writes each number before installing it.
3. **The order of two refusals.** A descriptor that is not open in one
   `SCM_RIGHTS` message, and more than 253 in total across later ones: Linux
   answers `EBADF` (it fetches each message's descriptors as it parses),
   here `EINVAL` (all are parsed first).
4. **A stream's credentials** are its connection's, not each write's
   (unchanged from before; shows only when several processes write one
   connection, or root states another process's credentials on a stream).

**Where:** 1: `kernel/src/ipc/unix_socket.rs` `take_garbage`; 2:
`kernel/src/syscall/linux.rs` `install_rights`; 3: `parse_send_control`
and `take_rights`; 4: `unix_socket::recv`.

**Proper fix:** 1: count the references a channel's queue holds in the
collector's reckoning -- one more kind of queue to scan, or a capability
transfer that refuses Unix sockets. 2: install each descriptor only after
its number is written -- the fd table needs a reserve-then-fill step for
that. 3: take each message's references while parsing. 4: per-write
credentials as marks, like the descriptors' (`stream_socket`).
