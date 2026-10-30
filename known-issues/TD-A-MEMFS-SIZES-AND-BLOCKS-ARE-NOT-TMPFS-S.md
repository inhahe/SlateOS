## TD-A-MEMFS-SIZES-AND-BLOCKS-ARE-NOT-TMPFS-S (lane A, 2026-10-30) — OPEN

**Status:** OPEN

**In short:** memfs, SlateOS's counterpart of Linux's tmpfs (`/tmp`, a
memfs `/`), reports every directory's size as 0 and every file's and
directory's block count as 0. tmpfs gives a directory 20 bytes for each
entry, "." and ".." among them (so 40 empty), and a file the blocks its
pages take. Programs rarely care, but `ls -l`, `du` and `stat` show other
numbers than Linux would, and a directory cannot be told from an empty
file by its size -- which is why a Linux driver process's test of reading
a directory as a file needs ext4's at `/mnt` (`kernel/src/linuxdrv.rs`'s
`firmware_runs`).

**Where.** `MemFsNode::size` and `to_file_meta`'s `blocks: 0` in
`kernel/src/fs/memfs.rs`.

**Why it was not simply changed.** It was, on 2026-10-30 (ef0a4f44b), and
reverted the same day after its review (Opus at max, Fable at xhigh)
found what else has to
change with it, all at once:
- `MemFs::stat`/`lstat` build the root's entry with a literal `size: 0`,
  so a mount root would disagree with `metadata`/`fstat`.
- `Vfs::readdir`'s `finish_listing` injects a mount point the parent lacks
  with `size: 0`, where it fetches the mounted root's `ino`: it should fetch
  its size the same way.
- posix's `fill_from_fsstat` (`posix/src/stat.rs`, lane D) makes
  `st_blocks` `ceil(st_size / 512)` whenever the kernel reports 0 blocks, so
  a 40-byte directory would get a block it does not have -- unless memfs
  reports real block counts and libc stops inventing them where a file
  system does.
- Kernel structs that copy a size and document it "0 for directories"
  (`fileops`, `fswalk`, `filepicker`, `pathbar`, `fileselect`, `index`,
  `conformance`, kshell `find`; kshell `du` adds every entry's size where
  Linux's adds blocks) count directory sizes in byte totals. ext4
  directories already break that assumption, so those totals are wrong for
  ext4 today too: a directory moves no bytes, whatever its size.

**The proper fix.** memfs sizes directories as tmpfs does and reports
blocks (a file's pages in 512-byte units, a directory's 0); the root
entry and `finish_listing` take both from the node; byte totals count
regular files only, and the docs say what each file system gives; and a
request to lane D for `fill_from_fsstat` to trust the kernel's block count
once every file system reports one. Then `firmware_runs` can read its own
`/tmp` directory as a file in every boot, and the boot test's one excused
skip (`LINUXDRV_NO_EXT4_SKIP` in `scripts/boot-test.sh`) can go.
