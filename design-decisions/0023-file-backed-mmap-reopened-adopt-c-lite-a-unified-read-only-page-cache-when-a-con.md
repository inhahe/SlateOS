## 23. File-backed `mmap` (reopened) — adopt **C-lite** (a unified *read-only* page cache) when a concrete consumer appears; writable `MAP_SHARED` writeback stays declined

> **LIFTED by §36** (2026-06-24) — the C-lite read-only page cache was built,
> and its precursor (stable VFS file identity) landed with it: `FileId`,
> `Vfs::file_identity`, and `mm::page_cache::get_or_fill`, which
> `proc/pcb.rs` already sources file-backed pages from. **Do not schedule the
> "precursor work that must land first" below — it exists.**

**Date:** 2026-06-14

**Decided by:** Operator (this reopened `open-questions.md` Q5; Claude proposed
the **C-lite** middle option — read-only cross-process page sharing without the
writable-`MAP_SHARED` writeback machinery — and the operator chose it). The
operator's words: *"Q5: yes, we'll go with C-lite, but if you don't want to
implement it now, document it wherever at what time we should implement it
later."* This **narrows §22**: §22's blanket "decline the unified page cache" no
longer holds — the *read-only* half is now planned (deferred). Everything else in
§22 stands: full option C and **writable `MAP_SHARED` writeback remain declined
indefinitely** (`ENOSYS`), and the shipped demand-paged `MAP_PRIVATE` path
(option B, §17) stays as-is meanwhile.

**What "C-lite" is.** A unified *read-only* page cache: pages of a file are
cached once and shared (read-only) across every process that maps or reads them,
giving two wins —
1. **Shared-library / read-only text dedup:** N processes mapping the same
   `libc`/`.text` share one set of physical frames instead of N copies.
2. **De-double-caching:** a file's pages live in one cache rather than being held
   both by the block buffer cache (`fs/cache.rs`) and per-mapping copies.

**What C-lite deliberately OMITS** (and why it's "lite"): the writable
`MAP_SHARED` path — dirty-page tracking, `msync`/writeback ordering, and
cross-process write coherence. That is the hard, hard-to-reverse half and it
stays declined (writable `MAP_SHARED` of a regular file keeps returning
`ENOSYS`, exactly as in §22). C-lite is read-only, so it needs no dirty/writeback
policy at all.

**Decision — implement later, not now.** Per the operator, C-lite is *adopted in
principle* but **not to be built immediately**. It is deferred until a concrete
consumer needs it.

**Trigger to implement (the "at what time" the operator asked for):** build
C-lite when the **first real consumer of cross-process read-only page sharing
appears** — in practice the **dynamic linker wanting shared-library `.text`
dedup** (multiple processes mapping the same `.so`). That is the moment the
memory-saving payoff becomes concrete rather than hypothetical. A secondary
trigger is any measured double-caching cost once the block buffer cache and
file-backed mappings are both heavily exercised.

**Precursor work that must land first.** C-lite needs **stable VFS file
identity** — a page cache is keyed by (file-identity, offset), and today
`FileMeta.ino` is `0` for memfs and FAT, so two mappings of "the same file"
cannot be recognised as such. Implementing stable inode/file-identity in
`fs/vfs.rs` is a prerequisite and should be scheduled as the first sub-task when
the trigger fires.

**Rationale (both sides):**
- *For deferring:* no consumer exists yet (the dynamic linker doesn't dedup
  shared text today), the precursor (stable file-identity) is itself a
  multi-file change, and building a cache with no client risks designing to the
  wrong access pattern. Slate OS is native-first, so the urgency is low.
- *For adopting (vs §22's full decline):* the read-only half is the *cheap,
  reversible, high-value* slice — it captures the one advantage the operator
  cared about ("saving memory for some programs") for the common shared-library
  case, without taking on the writable-shared hairiness that §22 rightly
  declined. C-lite is the Pareto-optimal point between B and full C.

**Consequences:**
- **known-issues.md TD22** reverts from "CLOSED (won't-fix gap 2)" to: **gap 1
  done (option B); gap-2 read-only sharing PLANNED (deferred, see §23); gap-2
  writable `MAP_SHARED` writeback won't-fix (`ENOSYS`).**
- A deferred-with-rationale entry is recorded in `todo.txt` with the trigger
  condition above.

**Where it will live:** `kernel/src/fs/vfs.rs` (stable file-identity precursor),
a new/extended page cache (likely unifying or fronting `kernel/src/fs/cache.rs`),
`kernel/src/mm/vma.rs` (`VmaKind::FileBacked` fault path sources pages from the
cache), `kernel/src/syscall/linux.rs` (`linux_file_mmap`). B's `FileBacked`
fault-path shape is already the right foundation — C-lite only changes the
*source* of each page (shared cache frame vs per-mapping `read_at`) and marks
shared frames read-only/refcounted.
