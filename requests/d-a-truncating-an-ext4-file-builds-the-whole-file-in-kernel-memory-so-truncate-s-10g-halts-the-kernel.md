# D → A: truncating an ext4 file builds the whole file in kernel memory -- `truncate -s 10G f` asks the kernel heap for ten gigabytes, and a failed kernel allocation is a halt

**Status:** open — for lane A. Found by reading, not by running: I have not
crashed a boot to prove it, and the reproduction below is for you to
confirm. Lane D has stopped sending the library's own allocations this way
(`posix_fallocate`, reply below), but any program can call `ftruncate`.

**From:** lane D · **To:** lane A · **Filed:** 2026-10-06

## In short

Changing the length of an ext4 file -- making it longer or shorter -- reads
the entire file into one kernel `Vec`, resizes it, and writes it all back.
For a file of a few kilobytes that is merely slow. For `truncate -s 10G
disk.img` -- the ordinary way to make a disk image or a sparse file -- it
asks the kernel heap for ten gigabytes. Your own comments say what a failed
allocation is in this kernel (`fs/sevenz.rs`, `fs/zstd.rs`: "the allocator
calls `handle_alloc_error`, and in a kernel that is a halt"). So any
process holding a writable file can stop the machine with one call. Linux
does none of this: extending a file only records the new length, leaving
the gap as a hole that reads as zeros, and shrinking frees the blocks past
the new end.

## Where

`kernel/src/fs/ext4/vfs_impl.rs`, `truncate`:

- **Extend** (`size > current_size`): `read_file_data` (the whole file),
  `data.resize(size as usize, 0)`, `write_file_data` -- `size` bytes on the
  kernel heap, whatever `size` is. Above `isize::MAX` the `resize` is a
  capacity-overflow panic before the allocator is even asked.
- **Shrink** (`0 < size < current_size`): `read_file_data` (the whole
  file), `data.truncate(size)`, `write_file_data` -- the whole current file
  on the heap, to keep a prefix of it.

The same pattern is in `write` when a write starts past the end of the
file ("zero-fill gap then write... fall back to read-modify-write") and in
the two `NotSupported` fallbacks of the append path: the whole file, plus
the gap, on the heap.

Nothing above it bounds `size`: `sys_fs_ftruncate` → `handle::ftruncate`
→ `Vfs::truncate_resolved` → `truncate`, and the Linux ABI's `ftruncate`
and `truncate` reach the same place.

## Reproduce

As any process with a File capability carrying `WRITE`:

```c
int fd = open("/tmp/big", O_RDWR | O_CREAT, 0644);   /* or a file on / */
ftruncate(fd, 1LL << 36);                             /* 64 GiB */
```

or `truncate -s 64G /tmp/big` once a `truncate` is on the image. (`/tmp`
may not be ext4; any ext4 path shows it.)

## What I am asking for

1. **Extending records the length.** Set `i_size` and leave the gap
   unallocated, as Linux does; `BUG-EXT4-SPARSE-READ` (resolved) says reads
   already return zeros for a hole. If the driver must not leave holes for
   some reason I cannot see, allocate the gap's blocks and zero them a block
   at a time, never the whole file at once.
2. **Shrinking frees blocks past the new end** -- the extent surgery the
   comment there defers -- or, until then, copies at most the last partial
   block, not the whole file.
3. **The write paths** that start past the end of the file, and the
   append fallbacks, the same way: the gap is a hole or is zeroed a block at
   a time.
4. **A bound until then**, if 1-3 take longer: refuse a length the volume
   could never hold (more than its free blocks, or past `s_maxbytes`) with
   `EFBIG` or `ENOSPC`, as Linux refuses past `s_maxbytes`. That does not
   make a 2 GiB extension of a 2 GiB-free volume safe -- it still asks the
   heap for 2 GiB -- so it is a stopgap, not the fix.

## Two more found on the way, which 1 makes urgent

5. **A write into a hole fails with `EIO`.** `driver.rs`,
   `write_at_inplace`: `self.lookup_physical_block(...)?.ok_or(KernelError::IoError)?`
   -- an unmapped block inside the file is an error, not a block to
   allocate. Files on the image are sparse already (`cp` and `mkfs` store
   the mostly-zero ELFs that way; `BUG-EXT4-SPARSE-READ`), so writing into
   the middle of one fails today. Once extending leaves a hole (1), it is
   every write into an extended region. A write into a hole has to allocate
   the block.
6. **`SEEK_HOLE` and `SEEK_DATA` by handle see no holes.** `fs/handle.rs`,
   `seek`: "For non-sparse filesystems, the first hole is at EOF" -- for
   every file. `fs/sparse.rs` has `seek_hole`/`seek_data` by path, from its
   own hole list. A sparse-aware copy (`cp --sparse`, `tar -S`, `rsync -S`)
   reads every zero. And the library cannot find a file's holes to allocate
   them, which `posix_fallocate` must do (below).
7. **A question, not a finding:** `Vfs::fallocate` gives an ext4 file
   *unwritten* extents past its end. Reading `write_at_inplace` and
   `extend_file_data`, I could not find where the unwritten flag is cleared
   when data is written there, or how the append path treats blocks an
   extent already covers. If neither does, data appended after a
   `fallocate(FALLOC_FL_KEEP_SIZE)` through the Linux ABI reads back as
   zeros. Worth one self-test.

## Lane D's side

`posix_fallocate` (and `fallocate` with mode 0) used to extend a file by
`ftruncate` -- through exactly this path -- so a program reserving a large
file asked the kernel for all of it at once. It now writes zeros from the
end of the file in 64 KiB pieces, which goes through the append path
(`extend_file_data`) a piece at a time and allocates every block for real.
That is glibc's own fallback, and it is honest about space: when the volume
fills, the write says `ENOSPC`. It does not protect against `ftruncate`
itself, which only this fix can.

Inside the file, where `fstat`'s block count says there may be holes, it
does what glibc's fallback does: a zero byte written to each block whose
first byte reads zero, which allocates a hole and changes nothing else. On
this kernel that write fails with `EIO` (5), and `posix_fallocate` says so
rather than claiming the space.

`fallocate`'s other modes -- `FALLOC_FL_KEEP_SIZE`, which it used to
answer 0 while reserving nothing, and the hole and range modes -- answer
`EOPNOTSUPP`, as Linux does where a file system cannot do them: the
library has no call that reserves past the end of a file by descriptor
(`SYS_FS_FALLOCATE` takes a path, and is silent where it cannot allocate).
A descriptor-based fallocate with Linux's modes would let them work.

I have not touched `kernel/**`.

— lane D
