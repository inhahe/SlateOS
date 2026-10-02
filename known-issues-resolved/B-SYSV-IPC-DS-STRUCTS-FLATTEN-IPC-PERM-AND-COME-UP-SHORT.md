## B-SYSV-IPC-DS-STRUCTS-FLATTEN-IPC-PERM-AND-COME-UP-SHORT (lane B, 2026-09-09) -- FIXED the same day

**In short:** the two System V IPC status structures inline the permission
block instead of nesting it, and end up smaller than the C library's.

| | ours | musl |
|---|---|---|
| `struct msqid_ds` | 80 | 120 |
| `struct shmid_ds` | 72 | 112 |

**Where.** `posix/src/sysv_msg.rs` and `posix/src/sysv_shm.rs`. Both open with
`msg_perm_uid`/`shm_perm_uid` and friends where C has a nested
`struct ipc_perm`, so the field *names* do not correspond either -- which is
why the gate checks their size only.

**The right fix is a real `struct ipc_perm`**, shared by both, which is also
what makes `msgctl`/`shmctl`'s `IPC_STAT` fill a caller's structure correctly.

**Fixed** exactly that way. `linux_ipc::IpcPerm` is the C library's structure
and sits beside the kernel's `Ipc64Perm` in the same file, deliberately: they
are the same 48 bytes and differ inside, and keeping them adjacent is what
stops one type being pressed into both roles -- which is how `sigaction` came
to carry the kernel's field order under a comment claiming glibc's.

**Both are now checked field by field**, not by size: the flattening was the
only reason the gate could not name their fields, and with it gone
`msg_perm`/`shm_perm` and the eight fields after each are asserted against
musl's own header.

**Worth noting what was already right.** `posix/src/linux_ipc_perm_types.rs`
has held the correct offsets -- key 0, uid 4 … mode 20, seq 24, size 48 -- the
whole time. The knowledge was in the tree; the two structures that needed it
just did not use it, which no test could see because none of them crossed a C
boundary.
