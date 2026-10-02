### A-LINUX-FSTAT-OF-A-FILE-WAS-MADE-UP -- 2026-10-01 -- FIXED the same day (lane A)

**In short:** a Linux program asking about a file it had open got an
answer the kernel made up. The size was 0, the permissions 0644, the owner
root, the inode the handle's number, and every timestamp "now". Asking
about the same file by name gave the true answer. glibc's `fstat` is
`newfstatat(fd, "", AT_EMPTY_PATH)`, which reached the made-up answer, as
did `statx` with `AT_EMPTY_PATH`.

**Who it hit:**
- anything that sizes a buffer or a mapping from `st_size` (`mmap` of
  "an empty file");
- `cp` and `install` copying a mode, which came out 0644, dropping
  execute bits;
- anything comparing `(st_dev, st_ino)` through a descriptor with the same
  pair from a path, which never matched;
- `make`-like tools reading `st_mtime` through a descriptor.

**Where:** `syscall/linux.rs`'s `fill_stat_for_fd` and `fill_statx_for_fd`.
They predate the VFS: an old `todo.txt` note says they "must report real
inode numbers, sizes ... when a real VFS lands". The path-based `stat` was
moved onto `FileMeta` then; these were not.

**Fixed:** a `HandleKind::File` descriptor now answers with its file's
metadata (`fs::handle::fstat`), through the same fill the path-based calls
use. The made-up answer stays for descriptors with no file behind them:
pipes, sockets, anonymous inodes.
