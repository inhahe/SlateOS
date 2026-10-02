## 36. Next large initiative (Q12) — build the operator-pre-approved C-lite read-only page cache now (lifts the §23 "not now")

**Date:** 2026-06-24

**Decided by:** Operator (this was `open-questions.md` Q12; the operator chose
option **E**). The operator's words: *"Q12: I guess let's go with E."*

**The decision.** With the bounded in-context work verified exhausted, the
operator selected the **C-lite unified read-only page cache** (§23 / Q5) as the
next large initiative. **This lifts the §23 "implement later, not now" hold** —
the trigger is now considered fired (the shared-library `.text` dedup payoff plus
the precursor being met), so the work is cleared to start. Scope is exactly §23's
C-lite: cache a file's pages once and share them **read-only** across every
process that maps/reads them (shared-library `.text` dedup + de-double-caching vs
the block buffer cache). **Writable `MAP_SHARED` writeback stays declined
(`ENOSYS`)** per §22/§23 — out of scope.

**Implementation plan (sub-tasks, in order).**
1. **Precursor — stable VFS file identity.** The cache is keyed by
   `(file-identity, offset)`. Verified 2026-06-24 that `FileMeta.ino` is now
   populated (ext4 real inode, FAT first-cluster, memfs `alloc_memfs_ino()`), so
   the precursor is substantially met; confirm every backend yields a stable
   non-zero identity and define the cache key around it.
2. **Read-only page cache structure.** A frame store keyed by
   `(file-identity, page-offset)` → refcounted physical frame, host-testable in
   isolation (insert/lookup/refcount/evict), zero boot-risk before any fault-path
   wiring. Likely unifies or fronts `kernel/src/fs/cache.rs`.
3. **Fault-path integration.** `VmaKind::FileBacked` faults source pages from the
   cache (shared read-only frame, refcount++) instead of a per-mapping `read_at`
   copy; mark shared frames read-only/refcounted; a private write CoW-copies out
   of the shared frame (existing CoW path derives flags correctly).
4. **Lifecycle.** Refcount on map/unmap/exit; eviction policy; coherence with the
   block buffer cache so a file's pages live in one place.

**Status (2026-06-30).** Sub-tasks 1–4 (the correctness slice) are **done**:
1. file identity (`FileId{fs_id,ino}` + `Vfs::file_identity`, commit 80cbbaa54);
2. the read-only `mm::page_cache` store (commits b18e45bfa, ad78a2b5c, model §37);
3. fault-path integration — whole-frame, frame-aligned `MAP_PRIVATE` FileBacked
   faults source shared read-only frames from the cache and CoW-copy on a private
   write; boot exercised it ~2158× with cross-process `FileId` sharing observed;
4. coherence — `invalidate_identity` wired into `write_at`/`write_file`/`truncate`/
   `remove`/replacing-rename (closes stale-data + inode-reuse, B-PAGECACHE-COHERENCE).
Shared-cache-page reclaim under memory pressure is also **done** (commit
f6003260c): `mm::page_cache::shrink(PressureLevel)` evicts idle cache frames
(refcount ≤ 1, no live mapper) proportional to pressure and is registered with
`mm::pressure` by `init()`; it fired under real critical pressure during boot
(freed 49 then 5 frames) with a clean BOOT_OK. Cache frames remain unregistered
with the swap clock/rmap by design (clean file pages reclaimed via the shrinker,
not swap — see `resolve_file_cached`).
**Remaining (performance, not correctness):** de-double-cache the page cache
against the block buffer cache (`fs/cache.rs`) so a page lives in one place.

**Rationale.** The §23-recorded Pareto-optimal slice: the cheap, reversible,
high-value read-only half that captures the memory-saving win for the common
shared-library case without the writable-shared hairiness §22/§23 declined.
Starting with the host-testable cache structure (sub-task 2) before the
boot-critical fault-path wiring (sub-task 3) keeps boot-risk out of early
increments.

**Where it lives:** `kernel/src/fs/vfs.rs` (identity precursor),
`kernel/src/fs/cache.rs` (unify/front the cache) or a new page-cache module,
`kernel/src/mm/` (`VmaKind::FileBacked` fault path), `kernel/src/syscall/linux.rs`
(`linux_file_mmap`).
