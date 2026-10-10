### [F] A native program reaches the display over TCP until the C library has channels -- 2026-10-10 -- **OPEN**

**Status:** OPEN -- **blocked by** lane D's
`requests/f-d-the-c-library-s-slateos-channel-calls.md`. Nothing is broken
meanwhile: no GUI program is on the image yet, and when one is, it falls
back to TCP on its own machine.

**In short:** a program on SlateOS should reach the compositor over a
SlateOS channel, because a channel's kernel says who connected -- which is
how the compositor tells the shell from any other program before obeying a
shell-only request. A native SlateOS program (every GUI program) can open
one only through the C library, and lane D's C library does not have the
five calls yet. Until it does, `gui/remote`'s channel transport answers
`ENOSYS` in a native program, the program talks to the compositor over TCP
on loopback, and over TCP the compositor cannot tell who is calling. Lane
C's credential service and file chooser, which register services of their
own through the same transport, cannot register at all.

**Where:** `gui/remote/src/channel.rs`, the `kernel` module's five functions
`service_register`, `service_accept`, `service_connect`,
`channel_peer_cred` and `channel_peer_has_key`, each `Err(ENOSYS)` today.

**Why it was this way before, and why not that:** the transport used to issue
the Linux table's SlateOS extensions (1001 to 1005) as `syscall`
instructions. Which table a `syscall` reaches is the process's, decided when
its binary is loaded, and a native program's table numbers those calls as
others -- so the "working" transport would have called whatever native
syscall 1003 is. Lane C found it
(`requests/c-f-linux-syscall-numbers-in-a-native-program-reach-other-calls.md`).
`ENOSYS` is what a kernel without channel descriptors says, and
`connect_failure_means_absent` falls back from it.

**The fix:** when lane D's C library has `slate_service_register`,
`slate_service_accept`, `slate_service_connect`, `slate_channel_peer_cred`
and `slate_channel_peer_has_key` (the Linux table's signatures), declare them
in `gui/remote/src/libc.rs` and call them from the five functions. The rest
of the transport -- `read`, `write`, `poll`, `fstat` and `close` on the
descriptors -- already goes through the C library.
