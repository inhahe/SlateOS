### A-NATIVE-PROGRAMS-CANNOT-PASS-DESCRIPTORS -- 2026-10-02 -- OPEN (lane A)

**Status:** OPEN (lane A) -- the kernel half fixed on lane-a-wip 2026-10-09,
awaiting a boot on main; the C library half is lane D's
(`requests/a-d-native-sendmsg-and-recvmsg-can-carry-descriptors.md`).

**In short:** a program on SlateOS's own C library cannot pass or receive
open files over a Unix-domain socket. The native socket calls (`SYS_UNIX_*`)
carry bytes and credentials but no descriptors, and a native receive of a
message that carries some releases them, as a Linux receive with no room for
them does. Linux programs can pass them. Nothing native is known to need it
yet; the C library's Wayland and D-Bus clients will.

**Where:** `kernel/src/syscall/handlers.rs` `sys_unix_send`/`sys_unix_recv`;
`kernel/src/ipc/passed.rs` (`Passed` holds a Linux `FdEntry`); lane D's
`posix/` for the C library's half.

**Proper fix:** a native send that takes a list of (resource type, handle)
pairs the caller holds -- checked against its `ipc_handles`, as capability
transfer checks -- carried as `Passed` in a native form, and a native receive
that registers each in the receiver's `ipc_handles` and hands back the pairs
for the C library to put in its own descriptor table. The C library keeps
the table in the program, so the kernel never sees descriptor numbers.

**Fix (kernel half, 2026-10-09):** the proper fix above, as built.
`SYS_UNIX_SENDMSG` (1169) takes a list of 16-byte records -- handle, type
(the spawn `fd_map`'s `fd_handle_type` codes), status flags -- each checked
against the sender's `ipc_handles` and taken as a `Passed` reference, last,
after every other argument, all or none; `ETOOMANYREFS` (new
`KernelError::TooManyReferences`, -305) past the sender's `RLIMIT_NOFILE` in
flight, as Linux's `too_many_unix_fds`. `SYS_UNIX_RECVMSG` (1170) registers
each in the receiver's `ipc_handles` (`pcb::native_install_passed`) and writes
the records back -- one reference per object, so a descriptor for an object
the receiver held already names the handle it had -- releasing what does not
fit or has no native type and reporting it as `UNIX_MSG_CTRUNC`. If the
records cannot be written back, nothing lands: each registration is undone
and its reference released (`pcb::native_uninstall_passed`), since a program
that was never told of a handle cannot close it. The descriptors travel as a
Linux program's do, so native and Linux programs pass them to each other.
`kernel/src/ipc/native_rights.rs`; boot-tested by `native_rights::self_test`
and `test_dispatch_unix_rights` (a pipe end between two processes through the
syscall layer, ending in a check that no reference leaked).
