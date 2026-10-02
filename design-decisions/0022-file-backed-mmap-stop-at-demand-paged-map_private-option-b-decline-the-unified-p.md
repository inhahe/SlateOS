## 22. File-backed `mmap` — stop at demand-paged `MAP_PRIVATE` (option B); decline the unified page cache (option C); writable `MAP_SHARED` stays `ENOSYS`

> **SUPERSEDED IN PART by §23 (2026-06-14).** The operator reopened Q5 and
> chose to adopt **C-lite** (a unified *read-only* page cache) when a concrete
> consumer appears — see §23. What still stands from §22: full option C and
> **writable `MAP_SHARED` writeback remain declined** (`ENOSYS` indefinitely),
> and option B stays shipped meanwhile. Only the blanket "decline the unified
> page cache" is narrowed: the read-only unified cache is now planned (deferred),
> the writable-shared half is not.

**Date:** 2026-06-14

**Decided by:** Operator (this was `open-questions.md` Q5; Claude built option B
autonomously — see §17 — and laid out the option-C fork; the operator declined
C). The operator's words: *"I guess A is the right option, since C is so hairy
and the only advantage appears to be saving memory for some linux programs and
our OS isn't supposed to be primarily a Linux system anyway and doesn't have
full Linux support."* The operator's intent is to **not build the big page-cache
fork (C)**; Claude kept the already-shipped demand-paged `MAP_PRIVATE` (B) rather
than reverting literally to eager-copy (A) — see "Interpretation" below.

**Context:**
File-backed `mmap(2)` had three candidate end-states (full detail in §17 and the
retired Q5 entry):
- **A.** eager private-copy (the pre-2026-06-14 status quo): allocate + `read_at`
  every frame at map time.
- **B.** demand-paged `MAP_PRIVATE` via `VmaKind::FileBacked` (shipped
  autonomously on 2026-06-14, §17): no frames up front, one page read per fault,
  copy-on-fault for private writes. Writable `MAP_SHARED` returns `ENOSYS`.
- **C.** a unified, file-identity-keyed page cache with demand paging +
  dirty-tracking + `msync`/`munmap` writeback — the real Linux model, enabling
  true cross-process shared writable maps and frame dedup, but a large
  multi-subsystem effort requiring a new stable VFS file-identity (`FileMeta.ino`
  is 0 for memfs/FAT today) and a double-cache-vs-unify call against the block
  buffer cache (`fs/cache.rs`).

**Decision — stop at B; do not build C.**
The unified page cache (C) and writable-`MAP_SHARED` writeback (gap 2 of
known-issues.md TD22) will **not** be built. Writable `MAP_SHARED` of a regular
file continues to return `ENOSYS` indefinitely. The shipped demand-paged
`MAP_PRIVATE` path (B) stays as the file-backed `mmap` implementation.

**Rationale (operator's):**
- C is a large, hard-to-reverse, multi-subsystem effort whose principal payoff is
  memory savings for *some* Linux programs plus writable shared file maps.
- Slate OS is **native-first** and does not aim for full Linux support, so paying
  C's complexity to chase complete `mmap` fidelity for Linux binaries is not
  worth it. `ENOSYS` for writable `MAP_SHARED` is an acceptable permanent answer
  for the Path Z target.

**Interpretation — why B is kept rather than a literal revert to A.** The
operator labelled the choice "A," but the reasoning was entirely about avoiding
**C's** hairiness, not about disliking B; the one advantage the operator named —
"saving memory for some programs" — is precisely what B *already delivers* for
`MAP_PRIVATE`, at low complexity, and B is already shipped, tested, correct, and
reversible. Reverting to literal eager-copy A would *discard* that memory saving
and replace working code with a strictly worse model — the opposite of the
operator's stated value ("saving memory"). So B (no C) is the faithful execution
of the operator's intent: no big page-cache project, keep the cheap demand-paging
win. **If the operator actually wants eager-copy A restored, it is a documented
one-spot revert** (drop the `FileBacked` arm, re-point `linux_file_mmap`'s
`MAP_PRIVATE` path at the eager loop — see §17 "How to reverse").

**Consequences:**
- **known-issues.md TD22** moves from PARTIAL to **CLOSED (won't-fix for gap 2)**:
  gap 1 (lazy population) is done via B; gap 2 (writable `MAP_SHARED` +
  cross-process coherence) is declined by operator decision.
- No stable VFS file-identity, no page cache, and no `fs/cache.rs` unify/double
  decision are needed.

**How to reverse:** if a concrete consumer ever needs writable `MAP_SHARED` or
cross-process file-map coherence, reopen the C fork (the three sub-questions in
the retired Q5 entry — gap-2 worth it?, double-cache vs unify?, stable
file-identity? — are the starting point). B's `VmaKind::FileBacked` fault-path
shape is already the right foundation for C; C would only change the *source* of
each page (page cache vs direct `read_at`) and add the shared/dirty policy.

**Where it lives:** same surface as §17 — `kernel/src/mm/vma.rs`
(`VmaKind::FileBacked`), `kernel/src/syscall/linux.rs` (`linux_file_mmap`),
`kernel/src/proc/pcb.rs` (handle lifecycle), `kernel/src/fs/procfs.rs` (maps
label). The declined C surface would additionally have touched
`kernel/src/fs/vfs.rs` (file-identity) and `kernel/src/fs/cache.rs`.
