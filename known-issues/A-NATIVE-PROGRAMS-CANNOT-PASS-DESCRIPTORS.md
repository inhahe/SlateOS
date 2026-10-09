### A-NATIVE-PROGRAMS-CANNOT-PASS-DESCRIPTORS -- 2026-10-02 -- OPEN (lane A)

**Status:** OPEN (lane A) -- the native half of descriptor passing, not yet
built.

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
