### A-SCM-RIGHTS-DIFFERENCES -- 2026-10-02 -- OPEN (lane A)

**Status:** OPEN (lane A) -- small differences from Linux left in descriptor
passing (design-decisions 1521). None is known to matter to a ported program.
Items 2 and 3 fixed on lane-a-wip 2026-10-09, awaiting a boot on main
(below); items 1 and 4 remain.

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

**Item 2 fixed (lane-a-wip, 2026-10-09).** When the numbers cannot be
written after all, `install_rights` takes each descriptor back out and
releases what it held, as a close would (`linux_close_taken`) -- but only
while its number still names the object put there
(`pcb::linux_fd_take_last_if`, judged and taken under one lock), since a
thread could have closed it and opened something else at that number in
between. Linux reaches the same end by writing each number before the
install. What remains of the difference is a window, between the install
and the taking back, in which another thread that guessed the number could
use the descriptor (or `dup` it, which then keeps it, under a number that
thread knows); the reserve-then-fill step above would close that too, and is
left for if it ever matters. The native receive got the same guarantee the
same day (`native_rights::land` undoes a landing its report could not
reach). Test: `linux::self_test_install_rights_take_back` (a failed write
installs nothing and leaks no reference; a working one keeps its
descriptor; a number naming something else is left alone).

**Item 3 fixed (lane-a-wip, 2026-10-09), and wider than written.** Linux's
`__scm_send` judges a send's control message by message, as it reads it --
a message that does not fit, a count past 253, each descriptor looked up
(`EBADF`), stated credentials checked (`EPERM`) -- all before the send's
size or address is looked at, and only `ETOOMANYREFS` comes after them
(`unix_attach_fds`). Here every number was collected first and looked up
after the size and the address, so beside item 3's case, a too-big datagram
carrying a descriptor not open answered `EMSGSIZE` where Linux answers
`EBADF`. `parse_send_control` now takes each reference and checks each
credential as it reads the message, and `attach_rights` keeps only the
in-flight check and the charge; a send refused after the parse gives the
references back as `SendControl` drops. Tests: the ring-3 `SCM_RIGHTS`
program's steps 0x26-0x28 (`elf::build_linux_scm_rights_test_elf`).
