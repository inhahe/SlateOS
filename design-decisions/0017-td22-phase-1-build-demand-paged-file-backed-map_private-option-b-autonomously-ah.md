## 17. TD22 Phase 1 — build demand-paged file-backed `MAP_PRIVATE` (option B) autonomously, ahead of the operator's option-C decision

**Date:** 2026-06-14

**Decided by:** Claude (autonomous) — this overrides the prior recommendation in
`open-questions.md` Q5, which had said "do B now, C later, but don't *build* B
until the operator settles C." I re-evaluated that rework risk, judged it low,
and proceeded. Reversible; the operator may overrule.

**Context:**
File-backed `mmap(2)` (`linux_file_mmap` in `kernel/src/syscall/linux.rs`) used
an **eager private-copy** model: at map time every 16 KiB frame was allocated,
the file bytes `read_at`-copied in, and the frame mapped via a `VmaKind::Fixed`
VMA. This wastes memory/latency on large or sparse maps (known-issues.md TD22
gap 1). The proper end-state is a unified page cache + writable `MAP_SHARED`
writeback (TD22 gap 2 / Q5 option C), which is a foundational, multi-subsystem
fork still deferred to the operator (needs a stable VFS file-identity, a
double-cache-vs-unify call against `fs/cache.rs`, etc.).

Q5's earlier recommendation was to hold off on even the `MAP_PRIVATE`
demand-paging half (option B) until C was settled, on the theory that C might
rework B.

**Decision:**
Build option B now: a `MAP_PRIVATE`, non-`MAP_FIXED` mmap of a regular file
registers a `VmaKind::FileBacked { handle, file_offset }` VMA and allocates **no
frames up front**. The page-fault handler resolves a fault by allocating a
zeroed frame, `read_at`-ing one page from the backing handle (tail past EOF
stays zero = Linux page zero-fill), and mapping it. Private writes copy-on-fault
to a per-process frame, never reaching the file. memfd-backed maps, read-only
`MAP_SHARED`, and `MAP_FIXED` overlays (ld.so segment loader) keep the eager
`VmaKind::Fixed` path. Writable `MAP_SHARED` still returns `ENOSYS` (gap 2,
unchanged).

The FileBacked VMA owns an independent reference on the open-file description
(via `fs::handle::dup_shared`, decoupled from the fd), with a full refcount
lifecycle: dup at mmap, per-VMA dup on fork, release on munmap / `MAP_FIXED`
split (net retain/release in `remove_vma_range`) / execve
(`reset_vmas_for_exec`) / process exit. All handle ops are deferred until the
`PROCESS_TABLE` lock is dropped, honoring the PROCESS_TABLE→OPEN_FILES lock
order. A pre-existing exec-VMA-staleness bug (execve tore down the address space
but never cleared the per-process `vmas` list) was fixed in passing via
`reset_vmas_for_exec`.

**Why this was safe to do without the operator (the rework-risk re-evaluation):**
- B's fault-path *shape* — a `VmaKind::FileBacked` VMA lazily populated by the
  fault handler — is exactly what C needs too. C only changes the *source* of
  the page (page cache vs direct `read_at`) and the private/shared policy.
- The only piece C might discard is the small, localized handle-refcount
  lifecycle (~60 lines, isolated in `pcb.rs`). Rewriting it under C is cheap.
- B is independently correct, not a temporary hack: `MAP_PRIVATE` may
  legitimately not observe later file writes, so demand-reading at fault time is
  *more* faithful than the eager snapshot.
- B is a strict improvement meanwhile (no eager whole-span copy) and fully
  reversible.

**Why not the alternatives:** leaving the eager copy (Q5 option A) keeps wasting
memory on big maps; waiting to build B until C is settled (the prior Q5 stance)
delays a correct, reversible improvement for a rework risk that on inspection is
small.

**How to reverse:** drop the `VmaKind::FileBacked` arm and re-point
`linux_file_mmap`'s `MAP_PRIVATE` path at the eager copy loop; the handle
lifecycle wiring in `pcb.rs` then becomes dead and can be deleted.

**Where it lives:** `kernel/src/mm/vma.rs` (`VmaKind::FileBacked`),
`kernel/src/syscall/linux.rs` (`linux_file_mmap`, `self_test_file_mmap`),
`kernel/src/proc/pcb.rs` (handle lifecycle: `add_vma`, `remove_vma`,
`remove_vma_range`, `reset_vmas_for_exec`, `try_resolve_fault`, `fork_create`,
`destroy`, `vma_release_backing`), `kernel/src/proc/spawn.rs` (exec reset),
`kernel/src/fs/procfs.rs` (maps label). Tracked as **known-issues.md TD22**
(now PARTIAL); option C remains **open-questions.md Q5**.
