## 37. C-lite page-cache refcount model — unify on the frame allocator's per-frame refcount (not a cache-owned mapper count)

**Date:** 2026-06-30

**Decided by:** Claude (operator-approved scope). The operator approved building
the C-lite cache (§36 / Q12=E); *how* the cache's frame lifetime integrates with
process teardown is an internal implementation detail with no user-visible
effect, so Claude resolved it. Recorded here because it is a genuine fork with
tradeoffs on both sides.

**The decision.** A cached page's lifetime is governed by the **frame
allocator's existing per-frame refcount** (`mm::frame::refcount` /
`unsafe ref_inc` / `free_frame`, the same mechanism CoW already uses), **not** by
a separate mapper-count inside the page cache. Concretely:

- The page cache holds **exactly one** frame reference per resident entry (the
  entry's presence in the map *is* that reference).
- When the `VmaKind::FileBacked` fault path maps a cached frame into a process,
  it bumps the frame refcount via `ref_inc` and maps the frame **read-only with
  the COW bit** (so a private write copies out of the shared frame via the
  existing CoW handler; writable `MAP_SHARED` stays `ENOSYS` per §23).
- Process unmap / exit frees mapped frames through the **standard `free_frame`
  teardown path with no changes** — it decrements the shared frame's refcount and
  only returns the frame to the allocator when the count hits zero.
- Eviction drops the cache's single reference via `free_frame`. To preserve
  dedup, eviction prefers entries whose frame refcount is exactly 1 (no live
  mappers); a still-mapped page can be evicted from the *index* but its frame
  survives for the mappers.
- "Is this page actively mapped?" is answered by `frame::refcount(frame) > 1`,
  not by a cache field.

**Alternatives considered.**

- **Cache-owned mapper refcount (rejected).** Have the cache count mappers
  itself (the sub-task-2 prototype's `refcount`/`release` API). *Con:* process
  teardown walks page tables and calls `free_frame` on every present user frame;
  a cache frame would then be decremented by the frame allocator while the cache
  *also* believed it owned the reference — a double-free / use-after-free unless
  the boot-critical teardown path is taught to special-case cache frames (skip
  `free_frame`, call `page_cache::release` instead). That is invasive and
  error-prone exactly where bugs are most dangerous. *Pro:* the cache could
  answer "actively mapped?" without taking the allocator lock.
- **Unify on the frame refcount (chosen).** *Pro:* reuses proven CoW + teardown
  machinery; **zero** changes to the exit/unmap free path; no double-bookkeeping;
  a private write already copies correctly because a shared file page always has
  refcount ≥ 2 (cache + mapper). *Con:* the sub-task-2 cache prototype's
  per-mapper `refcount`/`release`/`CachedPage` API is the wrong abstraction and
  was revised — the cache no longer tracks per-mapper references.

**Consequence.** The sub-task-2 `mm::page_cache` module (commit b18e45bfa) is
revised: `get_or_fill` returns the shared frame and the cache holds one
reference; per-mapper reference counting moves to the frame refcount; eviction
prefers unmapped frames. Only the **whole-frame** `FileBacked` fast path uses the
cache; the sub-page-straddling path (glibc's 4 KiB-packed segments, where one
16 KiB frame backs multiple VMAs at different file offsets) stays on the private
per-mapping read path — a single `(FileId, page_index)` key cannot describe a
frame shared across mismatched file offsets.

**Where it bites:** `kernel/src/mm/page_cache.rs` (refcount model + eviction),
`kernel/src/proc/pcb.rs` (`try_resolve_fault` whole-frame `FileBacked` path),
`kernel/src/mm/frame.rs` (`ref_inc`/`free_frame`, unchanged), the CoW handler in
`kernel/src/mm/cow.rs` (unchanged — already copies on write for refcount > 1).
