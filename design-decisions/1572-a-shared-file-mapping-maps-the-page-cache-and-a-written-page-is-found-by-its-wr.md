## 1572. A shared file mapping maps the page cache, and a written page is found by its write fault

**Date:** 2026-10-16 · **Decided by:** Claude (operator-approved scope: A-Q24,
§1566, "programs may change a file by writing to a shared mapping of it") ·
**Lane:** A

**In short:** §1566 decided that a program may change a file by writing to
memory the file is mapped into. This records how that is built. Every shared
mapping of a file, and every `read` and `write` of it, uses the same copy of
each page: the page cache's. The kernel learns that a mapped page was changed
the first time a program writes to it after it was last saved (the page is
mapped read-only until then, and that first write traps), and saves changed
pages to the file when asked (`msync`, `fsync`), when the last mapping goes,
and in the background. The other choices, and why not, are below.

### What exists, and what it lacks

- `mm::page_cache` keeps one frame per 16 KiB page of a file (`FileId`, page
  index), shared by refcount. `read(2)` copies out of it (§38); a whole-frame
  private mapping maps it read-only and copies on write (§36/§37).
- Every change to a file through the VFS invalidates the file's whole cache
  (`invalidate_identity`) after the change.
- A read-only `MAP_SHARED` is an eager private copy: a later `write(2)` to
  the file is not seen through it (LMDB's default mode, a read-only shared map
  beside `pwrite`, reads stale pages). A writable one is `ENOSYS`.
- `mm::rmap` is best effort (a fixed table that drops entries, and unmapping
  does not remove them), so it cannot be what finds a page's mappings.

### The design

1. **One copy.** A shared file mapping maps cache frames directly, part by
   part: the 4 KiB part at virtual `v` maps the cache frame of file page
   `fo / 16 KiB` at sub-offset `fo % 16 KiB`, where `fo` is `v`'s file
   offset. So any 4 KiB-aligned `MAP_FIXED` works; for a mapping the kernel
   places, it picks `v ≡ fo (mod 16 KiB)`, so each virtual frame maps one
   cache frame. A virtual frame holds one reference per distinct cache frame
   it maps into, the convention `unmap_user_range` already releases by. The
   PTEs carry `SHARED`: `fork` shares them, reclaim never swaps them.
2. **A write to the file reaches the mapped copy.** For a file with a live
   shared mapping, a VFS change updates the cached frames in place instead of
   invalidating them: a write copies its bytes into each resident frame it
   covers (after the filesystem write) and marks it dirty; a truncate zeroes
   a partial last page's tail and drops the pages wholly past the new end,
   first unmapping them everywhere (an access there is then `SIGBUS`, as on
   Linux); unlinking or renaming over the file leaves its pages, which the
   mappings' open handles keep alive. Files with no shared mapping keep
   today's invalidation.
3. **A fill cannot install stale bytes.** Each cached file has a change
   generation, bumped by every change in (2). A fill that read the file
   before a change and inserts after it is discarded and refilled. (This race
   exists today: a fill that reads before a write and inserts after the
   write's invalidation keeps the old bytes.)
4. **Dirty pages are found by their first write.** A shared writable mapping
   maps a clean page read-only. A write traps; the fault marks the cache
   entry dirty and makes that PTE writable. Saving a page first takes the
   write permission back from every PTE mapping it, flushes the TLBs, then
   copies the frame out and writes it, so a store after the copy traps and
   dirties the page again. Linux does the same (`page_mkclean`,
   `page_mkwrite`). The mappings are found object-first, as Linux's `i_mmap`
   finds them: each process whose VMAs map the file, its address space held
   by `AsPin` while its page tables are edited.
5. **When pages are saved.** `msync(MS_SYNC)` saves the dirty pages of its
   range before returning, `MS_ASYNC` schedules them; `fsync`/`fdatasync`
   save every dirty page of the file; the last shared mapping of a file going
   (`munmap`, exit, `exec`) saves its dirty pages; a background pass saves
   pages dirty for longer than a few seconds. A dirty page is never evicted
   (the shrinker passes it). Bytes past the end of the file in the last page
   are never written.
6. **What saves them.** A shared-writable file keeps a handle opened for
   writing (a reference on one of its mappings' handles) for as long as it
   has dirty pages or a writable mapping; saving writes through the
   filesystem's own write path, which does not touch the cache.
7. **Both ABIs.** Linux `mmap` and the native `SYS_MMAP_FILE` take the same
   path. `mprotect` adding `PROT_WRITE` to a shared file mapping needs the
   handle to be writable (`EACCES` otherwise, as Linux).
8. **No stable identity, no shared writes.** A file whose filesystem gives
   no stable identity (FAT, ISO 9660: `FileId` is `None`) has no cache, so a
   writable shared mapping of it stays refused (`ENODEV`, which Linux
   answers for a file that cannot be mapped that way).

### Built in three steps, each with its tests

1. The cache holds dirty pages and a change generation; VFS changes to a
   file with a shared mapping update it in place; a read-only `MAP_SHARED`
   maps the cache (so `pwrite` is seen through it).
2. Writable `MAP_SHARED` for both ABIs: the write fault, saving on `msync`,
   `fsync` and the last unmapping, truncate's unmapping and `SIGBUS`.
3. The background pass, and `/proc/meminfo`'s `Dirty:` and `Writeback:`.

### The alternatives not taken

- **Hardware dirty bits instead of the write fault.** Saving would read and
  clear the dirty bit of every PTE of every mapping, four per frame here,
  and a page dirtied through a stale TLB entry between the read and the
  clear is lost unless the walk flushes first anyway. The write fault is
  one trap per page per save, and is what Linux uses for shared file pages.
- **Treat every page mapped writable as dirty.** Correct, and simple, but
  `msync` would write every resident page of the range every time: a
  database with a writable map of gigabytes rewrites all of it at every
  commit.
- **A full write-back cache for every file** (Linux's model: `write` lands in
  the cache, the filesystem sees it later). The end state, perhaps, but it
  changes every file's durability and every filesystem's write path at once;
  the in-place update of (2) gives shared mappings their coherence without
  that.
- **`mm::rmap` to find the mappings.** It is a fixed-size, best-effort table
  for compaction, and registering cache frames there would also make
  compaction move them under the cache.
