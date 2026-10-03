## 38. De-double-cache file data (Q13) — page-cache-primary (option A): the page cache is the single cache for regular-file data; the buffer cache caches only filesystem metadata

**Date:** 2026-06-30

**Decided by:** Operator (this was `open-questions.md` Q13; the operator chose
option **A**). The operator's words: *"Q13: A."* Claude recommended A as the
correct long-term end-state. (Q12=§36's one remaining performance item.)

**The decision.** File *data* I/O is cached in exactly **one** place: the
**page cache** (`mm::page_cache`, 16 KiB pages). The block buffer cache
(`fs/cache.rs`, 512 B sectors) is demoted to caching only filesystem
**metadata** — superblock, block/inode bitmaps, inode tables, directory blocks,
journal — never regular-file data pages. Regular-file `read(2)`/`write(2)` **and**
mmap all source/sink their data through the page cache, which unifies `read(2)`
and mmap coherence for free (one shared frame, no separate invalidation needed
for the read path). Today (status quo before this change) a mmap'd file page is
cached as 32 sectors in the buffer cache *and* as one 16 KiB page in the page
cache — this change removes that double-caching.

**Alternatives considered (from Q13).**
- **(B) Read-through + drop-behind (rejected).** Keep the buffer cache as the
  device cache but mark the sectors the page-cache fill consumed as immediately
  evictable / bypass the buffer cache for whole-page file reads. *Pro:* small,
  localized, no FS-path refactor. *Con:* doesn't truly unify — a concurrent
  `read(2)` re-populates the buffer cache; read/mmap coherence still leans on the
  §36 invalidation hooks rather than a genuinely shared frame. A stepping-stone
  that A subsumes, so going straight to A avoids throwaway work.
- **(C) Leave as-is (rejected).** Accept the double-caching. *Pro:* zero risk.
  *Con:* memory wasted on hot mmap'd files; not the §36 end-state.

**Rationale.** Option A is the canonical, proven (Linux-like) design: truly one
copy of a file's data, and `read(2)`/mmap coherence falls out of the shared
frame for free. The cost is a real FS-data-path refactor (route metadata vs.
data correctly per filesystem) — the largest blast radius of the three — but it
is the correct end-state and the operator picked it directly, so there is no
reason to build B first as a throwaway.

**Where it bites.** `kernel/src/mm/page_cache.rs` (`get_or_fill` fill path),
`kernel/src/fs/cache.rs` (buffer cache — restrict to metadata),
`kernel/src/fs/handle.rs` / `kernel/src/fs/vfs.rs` (`read_at`/`write_at`
routing through the page cache), and the ext4/FAT data read/write paths under
`kernel/src/fs/` and `fs/` (route data through the page cache, metadata through
the buffer cache).

### Implementation sub-design (2026-06-30)

**Decided by:** Claude (operator-approved scope — the operator chose option A;
these are the implementation-level sub-decisions made while building it). All
reversible.

The refactor landed in four increments, all preserving the **per-block
read/write cache-path symmetry** invariant: for any one physical block, reads
and writes use the *same* cache path, or a read-after-write serves stale bytes.

1. **Two buffer-cache-bypassing sector primitives** (`fs/cache.rs`):
   `read_sector_uncached` (serves a *dirty* buffer-cache hit if present —
   that's legitimate metadata pending writeback — else drops a clean hit and
   reads straight from `blkdev`) and `write_sector_uncached` (writes straight
   to `blkdev`, then `invalidate_range` drops any buffer-cache alias). Plus
   ext4 `BlockReader` data methods (`read_data_block`/`write_data_block`/
   `invalidate_block`) and `read_block_classed`/`write_block_classed`
   dispatchers taking an `is_file_data: bool`.

2. **Block-reuse coherence** (`fs/ext4/balloc.rs`): `free_block` now calls
   `reader.invalidate_block` on the freed LBAs. Directory/extent-tree blocks
   are allocated from the same data-region pool; when a freed metadata block is
   later reused as file data (written via the bypass path), a stale *dirty*
   metadata buffer-cache entry would otherwise win. This mirrors Linux's
   `clean_bdev_aliases`.

3. **Data-vs-metadata classification by inode mode** (`fs/ext4/driver.rs`):
   `inode_holds_file_data(inode)` = `(i_mode & S_IFMT) == S_IFREG`. The shared
   leaf read/write helpers (`read_file_data`, `write_file_data`,
   `write_to_existing_blocks`, the extent/indirect leaf readers) are used by
   **both** directories and regular files, so the data/metadata split is keyed
   on the inode mode threaded through as `is_file_data`, *not* on the function.
   A blanket switch would have read directories (written via the buffer cache)
   back through the bypass path → stale directory reads. Extent-tree *internal*
   nodes, htree directory blocks, xattr blocks, bitmaps and inode tables stay on
   the buffer cache (metadata).

4. **`read(2)`/`read_file` routed through the page cache** (`fs/vfs.rs`):
   `Vfs::read_at` and `Vfs::read_file` now serve **stable-identity regular
   files** (`ino != 0`: ext4, memfs) from `mm::page_cache` via a new
   `page_cache::read_through` (splits an arbitrary `[offset,len)` into covering
   16 KiB pages, fills misses from the FS *data* path, copies out, drops each
   caller ref). Non-regular files and no-stable-identity filesystems (FAT,
   ISO9660, pseudo-fs — they keep their own caching) fall back to the
   per-filesystem read unchanged. This is what restores caching for `read(2)`
   after increment 3 removed regular-file data from the buffer cache — and it
   unifies `read(2)`/`mmap` on one shared frame.

   - **Reentrancy fix.** The `mmap` fault fill (`proc/pcb.rs resolve_file_cached`)
     previously filled via `handle::read_at`, which now routes through
     `get_or_fill` → it would recurse on the very page being filled. New
     `Vfs::read_at_uncached` / `handle::read_at_uncached` read straight from the
     FS data path (bypassing *both* caches); the mmap fill and `read_through`'s
     fill closure use them. No lock nesting: the page-cache lock is always
     dropped before a fill closure takes the VFS lock (order is VFS→drop,
     cache→drop, VFS-fill→drop — never simultaneous).

**Known minor inefficiency (logged):** memfs/tmpfs files now hold their data
both in memfs's own store *and* (when read/mmap'd) in the page cache — Linux's
tmpfs *is* the page cache, so this is a double-store for tmpfs specifically. It
is coherent (writes invalidate) and was already true for `mmap` of memfs before
this change; not worth special-casing now.
