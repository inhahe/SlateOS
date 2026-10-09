### A-NO-NAMED-PIPES-AND-EXT4-DEVICE-NODES-READ-AS-FILES -- 2026-10-02 -- FIXED 2026-10-09 (lane A)

**Status:** FIXED 2026-10-09 -- both halves fixed on lane-a-wip, on main since b083cfeca (lane A's publish of 2026-10-09, boot-tested green at 3d83e0264): the ext4 device nodes 2026-10-03 (character and block device inodes read as devices with their numbers, and refuse reads with ENXIO), and named pipes 2026-10-08 (`ipc::fifo`, design-decisions 1551: `mknod(S_IFIFO)` on memfs and ext4, opens with POSIX's waits, an ext4 FIFO read as one; checked by `self_test_linux_fifo` and `ipc::fifo::self_test`). Native programs need lane D's C library to use the two new calls (`requests/a-d-named-pipes-have-native-calls.md`). Stamp it FIXED and move it once a boot on main has run those tests.

**In short:** two kinds of special file are missing or misreported.
- **Named pipes (FIFOs) do not exist.** `mkfifo` and `mknod(path, S_IFIFO)`
  are refused with `EPERM`, which reads as "not allowed" when the truth is
  "not built". Linux makes one for any caller who may write the directory.
  Shell scripts that pass data between two programs through a FIFO, and
  some build systems' job servers, fail at the first step.
- **ext4 device, FIFO and other special nodes read as regular files.** A
  disk image's `/dev/sda` or a FIFO stored on ext4 shows in a listing and in
  `stat` as an empty regular file. (Sockets' nodes are read correctly since
  2026-10-02.)

**Where:** `kernel/src/syscall/linux.rs` `sys_mknod_common`;
`kernel/src/fs/ext4/vfs_impl.rs` `dir_type_to_entry_type` and
`mode_to_entry_type`, which fall back to `EntryType::File`.

**Proper fix:** an `EntryType::Fifo`, with FIFO nodes made as socket nodes
are (`FileSystem::mknod_socket`'s shape), and opening one attaching the
opener to a kernel pipe found by the node's identity -- as a connect to a
socket's node finds the bound socket. ext4 then maps `CHRDEV`/`BLKDEV`/`FIFO`
to their types; a device node on ext4 also needs `open` to refuse it (there
is no device behind a stored major/minor here) rather than read it as a
file.
