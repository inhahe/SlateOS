### A-NO-NAMED-PIPES-AND-EXT4-DEVICE-NODES-READ-AS-FILES -- 2026-10-02 -- OPEN (lane A)

**Status:** OPEN (lane A) -- the ext4 device-node half fixed on lane-a-wip 2026-10-03, awaiting a boot on main: character and block device inodes read as devices with their numbers, and refuse reads with ENXIO. FIFOs remain.

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
