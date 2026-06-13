# Open Questions — Operator Decision Queue

Decisions that genuinely need the human operator: architectural forks,
user-visible policies, and tradeoffs with no obviously-correct answer that
Claude has **deferred** rather than resolved autonomously.

This file is distinct from:

- **`design-decisions.md`** — decisions already *made* (each marked with who
  decided it). When the operator answers a question here, move it there as a
  `Decided by: Operator` entry and delete it from this file.
- **`known-issues.md`** — bugs and accumulated technical debt.
- **`todo.txt`** — the working scratchpad / judgment-call log.

Format for each entry:

- **Question** — the decision to be made.
- **Options** — each with its pros and cons.
- **Claude's recommendation** — if there is a defensible default (and what
  Claude is doing in the meantime).
- **Where it bites** — files/symbols affected, so the resolution can be applied.
- **Status** — `OPEN` until the operator decides.

---

Two low-stakes confirmations are outstanding (from the Q1/Q2 follow-ups — full
reasoning in `operator-answers-2026-06-13.md`; neither blocks anything, both are
already the shipped behavior, so these are "say so if you want them changed"):

- **Q1 confirm:** keep returning success (option A) for the NUMA mempolicy
  syscalls on our single-node hardware? (recommended)
- **Q2 confirm:** keep the shipped commit-policy defaults — native
  strict-commit, Linux lazy/overcommit? (recommended)

One substantive question is open below: **Q5** — how far to take the proper fix
for file-backed `mmap` (eager copy vs demand paging vs a full unified page
cache), which is architecturally coupled and so deferred to the operator. It
does **not** block Path Z (the eager-copy model already serves the
dynamic-linker + data-map cases), so work continues on other unblocked tasks
meanwhile.

---

Recently resolved (see `design-decisions.md` for the full rationale):

- The coreutils "which set is canonical?" question — resolved 2026-06-12;
  standalone per-tool crates are canonical (§8).
- Q1 `set_mempolicy_home_node` / NUMA mempolicy on UMA — resolved 2026-06-13;
  keep the UMA no-op returning 0, option A (§10).
- Q2 `/proc/sys/vm/overcommit_memory` & memory-commit policy — resolved
  2026-06-13; keep `vm/` omitted now, build the configurable both-strategies
  model (Option 5) as the end-state; do not add `CAP_SYS_ADMIN` as a native
  capability — map it to fine-grained native caps (new `admin.memory_policy`
  for the system-wide overcommit knob) (§11).
- Q3 next major initiative — resolved 2026-06-13; terminal/dev before GUI,
  GCC/CMake/Make toolchain first, CPython then fastpy (§9).
- Q4 toolchain on Slate OS: run-prebuilt-Linux vs native-port — resolved
  2026-06-13; **Path Z** (run prebuilt Linux toolchain binaries on the Linux-ABI
  layer now, native-port selectively later), native-first/no-leak kept
  inviolate, clang green-lit for install (§12).

---

## Q5 — File-backed `mmap`: keep the eager private-copy model, or build demand paging + a unified page cache? — OPEN (2026-06-13)

**Background.** File-backed `mmap(2)` (`linux_file_mmap` in
`kernel/src/syscall/linux.rs`) currently uses an **eager private-copy** model:
at map time every 16 KiB frame is allocated, the file bytes are `read_at`-copied
in, and the frame is mapped (a `VmaKind::Fixed` VMA is registered). This works
for `ld.so` shared-object loading and ordinary `MAP_PRIVATE` data maps — the
Path X/Z target — and is fully tested (ring-3 end-to-end, offset 0 + nonzero
offset). Two gaps remain (tracked as **known-issues.md TD22**):

1. **No lazy population** — a large or sparse map allocates+copies the whole
   span up front (memory cost + latency).
2. **Writable `MAP_SHARED` returns `ENOSYS`** — we never write modified pages
   back, and two processes mapping the same file get independent private copies
   (no shared coherence).

**Question.** How far do we take the proper fix, and with what architecture?
The two gaps have very different cost/benefit, and the implementation of gap 1
is **coupled** to the architecture chosen for gap 2 (see "Where it bites").

**Options.**

- **A. Leave it as eager private-copy (status quo).**
  - *Pros:* already works for the dynamic-linker + data-map cases that Path X/Z
    needs; simplest; no MM/FS-boundary churn; no new lifetime hazards.
  - *Cons:* wastes memory on big maps; writable `MAP_SHARED` programs (some
    databases, `mmap`-based logging/IPC) fail with `ENOSYS`.

- **B. Demand-page `MAP_PRIVATE` only (gap 1), no shared cache.**
  Add a `VmaKind::FileBacked` + a per-process VMA→backing table holding a
  `dup_shared`'d `fs::handle` + file offset; resolve faults by reading one page
  on demand; keep `MAP_SHARED`-writable as `ENOSYS`.
  - *Pros:* fixes the memory/latency gap; clearly correct (MAP_PRIVATE may
    legitimately not observe later file writes); reversible.
  - *Cons:* laborious + security-sensitive: the backing handle must be
    refcount-tracked through `remove_vma_range` VMA splits (ld.so overlays
    `MAP_FIXED` onto sub-ranges, splitting a file-backed reservation), `fork`,
    and process teardown — a handle leak or double-close here runs for every
    dynamically-linked process. Does **not** fix writable `MAP_SHARED`.

- **C. Full unified page cache + demand paging + writeback (gap 1 + gap 2).**
  Introduce a file-level page cache keyed by a stable file identity, consulted
  by the fault handler (CoW for private, shared for shared) and by the VFS
  read/write path; add dirty tracking + `msync`/`munmap` writeback.
  - *Pros:* the real Linux model; fixes both gaps; enables true cross-process
    shared maps and write-back; dedups frames across processes.
  - *Cons:* large multi-subsystem effort. **Requires a stable per-file identity
    the VFS does not yet provide** (`FileMeta.ino` is 0 for memfs/FAT, so it
    can't key a cache) — a precursor refactor touching every filesystem. Also
    forces a decision on the page-cache ↔ existing block buffer cache
    (`fs/cache.rs`) relationship: **double-cache** (file cache above FS,
    independent — simpler, reversible) vs **unify** (single cache, block cache
    as a view — memory-efficient, complex, hard to reverse).

**Claude's recommendation.** Pursue **B now, C later** — but B's VMA-backing
representation should not be built until the operator settles C's architecture,
because C may make the backing a *page-cache reference keyed by file identity*
rather than a *raw per-VMA handle*, which would throw away B's
refcount-sensitive lifecycle code (premature-abstraction / rework risk). So the
specific decisions I need:
  1. Is gap 2 (writable `MAP_SHARED` + cross-process coherence) worth building
     at all for the Path Z target, or is `ENOSYS` acceptable indefinitely?
  2. If yes to a page cache: **double-cache vs unify** with the block buffer
     cache?
  3. Endorse adding a stable VFS file-identity (the precursor C needs)?

In the meantime I am **not** starting this; I'm clearing unrelated unblocked
debts (e.g. the ALSA STATUS ioctl, TD10).

**Where it bites.** `kernel/src/syscall/linux.rs` (`linux_file_mmap`,
`unmap_user_range`, `linux_file_mmap_rollback`); `kernel/src/mm/vma.rs`
(`VmaKind`); `kernel/src/proc/pcb.rs` (`remove_vma_range`, `try_resolve_fault`,
the VMA list + a new backing table, fork clone, teardown); `kernel/src/fs/vfs.rs`
(`FileMeta.ino` / a new stable file-identity); `kernel/src/fs/cache.rs` (the
block buffer cache C would relate to).

**Status:** OPEN.
