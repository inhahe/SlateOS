### B-PAGECACHE-COHERENCE. Read-only page cache invalidation on FS mutations — FIXED 2026-06-30 (de-double-cache vs. buffer cache still pending)

**Resolution (2026-06-30):** the two correctness gaps below are now
closed. `mm::page_cache::invalidate_identity(fs_id, ino)` is wired into
the VFS mutation paths — `Vfs::write_at`, `Vfs::write_file`,
`Vfs::truncate`, `Vfs::remove`, and replacing same-mount `Vfs::rename`
— via the `cache_identity()` helper, which captures the file's
`(fs_id, ino)` under the held VFS lock (gated on a single relaxed
`is_populated()` atomic so the write path pays ~nothing when nothing is
cached). `remove` and the replacing-rename capture identity *before* the
inode is freed, closing the inode-reuse hole; the others capture after
the content change. Verified by boot self-test check 8 (is_populated +
invalidate_identity) and a green BOOT_OK.

**Shrinker (sub-task 4 eviction) landed 2026-06-30.**
`mm::page_cache::shrink(PressureLevel)` evicts *idle* cached pages
(refcount ≤ 1, i.e. no live mapper) proportional to the pressure level
(Low 25% / Medium 50% / Critical 90%), registered with `mm::pressure`
by `mm::page_cache::init()` (called from `kernel_main`). Verified by
boot self-test check 9 (shrink spares live, evicts idle) *and* by the
shrinker actually firing under real critical pressure during boot —
serial shows `[pressure] page_cache freed 49 objects (level=critical)`
then `freed 5 objects`, with BOOT_OK reached cleanly. Freeing 54 frames
under live pressure with no fault is a strong exercise of the
freed-while-mapped hypothesis: a mapped cache page always has
refcount ≥ 2 (cache entry + each PTE; `map_frame` does not bump
refcount, so the `get_or_fill` caller ref *becomes* the PTE ref), so
the shrinker's `refcount <= 1` gate never selects a mapped frame.

**Still pending (performance, not correctness — §36 sub-task 4 tail):**
de-double-cache the page cache against the block buffer cache
(`fs/cache.rs`) so a page does not live in both. Tracked as a follow-up;
not a bug.

The original write-up (now resolved for the correctness parts):



**Where:** `kernel/src/mm/page_cache.rs` (the cache) + the VFS/handle
write/truncate/unlink/rename paths (`kernel/src/fs/handle.rs`,
`kernel/src/fs/vfs.rs`, and the relevant syscall translators in
`kernel/src/syscall/linux.rs`).

**What it is:** sub-task 3 (commit wiring the FileBacked fault path to
`page_cache::get_or_fill`) populates the cache from mmap faults but does
**not** yet invalidate cached pages when the backing file changes. Two
correctness gaps result:

1. **Stale data after write/truncate.** If process A `mmap`s a file
   (pages enter the cache) and process B `write(2)`s or `ftruncate(2)`s
   that same file, A keeps seeing the *old* bytes through its mapping
   (and any later mmap of the file gets the cached stale page). The
   cache is read-only by design (writable MAP_SHARED writeback stays
   ENOSYS, §23), but read-side coherence with `write(2)` is still
   required and is missing.

2. **Inode-number reuse.** The cache key is `FileId { fs_id, ino }`.
   `fs_id` is monotonic per-mount (never reused), but `ino` **can** be
   reused within a mount after `unlink`. If file X (ino 53) is cached,
   unlinked, and a new file Y reuses ino 53, a fault on Y would be
   served X's stale pages. (`fs_id` prevents *cross-mount* collisions
   only.)

**Effect:** wrong file contents observed through a file mapping after a
concurrent write/truncate, or after unlink+recreate reuses an inode.
Not hit on the boot path (programs mmap read-only shared objects they
don't concurrently rewrite), which is why boot is green — but it is a
real correctness bug for general workloads.

**Proper fix (sub-task 4):** wire cache invalidation to FS mutations:
`page_cache::invalidate_file(file_id)` (or a page-range invalidate) on
`write`/`pwrite` that extends/overwrites a regular file, on `truncate`/
`ftruncate`, and on `unlink`/`rename` that drops/replaces an inode.
Resolve the `FileId` cheaply at the mutation site (the handle/path is
already known). Keep it cheap when nothing is cached (the
BTreeMap-range invalidate already returns 0 fast for an absent file).
Also de-double-cache against the block buffer cache (`fs/cache.rs`) per
§36 sub-task 4. Until this lands, the page cache is only safe for the
read-mostly mmap workloads the boot path exercises.

**Discovered/created:** 2026-06-30 (completing sub-task 3 without
sub-task 4's coherence wiring).
