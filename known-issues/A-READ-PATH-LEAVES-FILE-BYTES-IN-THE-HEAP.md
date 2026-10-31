## A-READ-PATH-LEAVES-FILE-BYTES-IN-THE-HEAP (lane A, 2026-10-31) — OPEN

**Status:** OPEN

**In short:** when a process reads a file, the kernel copies the bytes
through buffers of its own heap on the way, and frees those buffers without
zeroing them. A secret read from a file -- the WiFi passphrase the test PC's
driver reads at boot (design-decisions 1596) -- so stays in freed kernel
memory until the heap gives that memory out again. No process can read freed
kernel memory; a kernel bug that leaked it, or a memory dump, could.

**Where.** `kernel/src/fs/memfs.rs`: `FileSystem::read_at` and its kin
return a fresh `Vec<u8>` of the bytes asked for, which the read syscall
copies out to the process and drops. Other filesystems' reads do the same.
The memfs's own copy is zeroed when a filesystem of `memfs::mount_wiped`
is dropped, and the boot module's by `boot::wipe_modules`; the read path's
copies are not.

**The proper fix.** One of:
- a read path that copies from the filesystem's storage straight into the
  process's buffer, with no heap copy between (also faster for large
  reads); or
- a filesystem flag, as `mount_wiped` sets, that has the read path zero
  its buffer before freeing it; or
- the heap zeroing every allocation as it is freed (Linux's
  `init_on_free=1`), which costs every free a write of its size.

Trigger: a second secret read from a file by a kernel-started process, or
work on the read path's copies for speed.
