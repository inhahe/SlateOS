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

One substantive question is **partially** open below: **Q5** — how far to take
the proper fix for file-backed `mmap`. Its first half (demand-paged
`MAP_PRIVATE`, option B) was built autonomously on 2026-06-14 (TD22 Phase 1);
what remains open for the operator is the **unified page cache + writable
`MAP_SHARED`** fork (option C). It does **not** block Path Z (demand-paged
`MAP_PRIVATE` + the eager-copy fallback already serve the dynamic-linker + data-
map cases), so work continues on other unblocked tasks meanwhile.

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

**UPDATE 2026-06-14 — option B (Phase 1) was built autonomously.** The original
recommendation here was "B now, C later, but don't build B until the operator
settles C" (rework risk: C might replace B's raw per-VMA handle with a
page-cache reference). I re-evaluated that risk and judged it **low enough to
proceed with B**, because:
  - B's fault-path *shape* (a `VmaKind::FileBacked` VMA, lazily populated by the
    fault handler) is exactly what C needs too — C only changes the *source* of
    the page (page cache vs direct `read_at`) and the private/shared policy.
  - The only piece C might discard is the small, localized handle-refcount
    lifecycle (mmap/fork/split/exit). That code is ~60 lines and isolated in
    `pcb.rs`; rewriting it under C is cheap, and meanwhile B is a strict
    improvement (no more eager whole-span copy) and fully reversible.
  - B is independently correct: `MAP_PRIVATE` may legitimately not observe later
    file writes, so demand-reading at fault time is *more* faithful than the
    eager snapshot, not a temporary hack.
So B shipped (see `design-decisions.md`, "Decided by: Claude (autonomous)") and
**known-issues.md TD22** is now PARTIAL. **If the operator disagrees with making
this call without them, B is easy to revert** (drop the `FileBacked` arm and
re-point `linux_file_mmap`'s `MAP_PRIVATE` path at the eager loop).

**What's still genuinely OPEN here is C only** — the page-cache fork. The
decisions I still need from the operator:
  1. Is gap 2 (writable `MAP_SHARED` + cross-process coherence) worth building
     at all for the Path Z target, or is `ENOSYS` acceptable indefinitely?
  2. If yes to a page cache: **double-cache vs unify** with the block buffer
     cache (`fs/cache.rs`)?
  3. Endorse adding a stable VFS file-identity (the precursor C needs —
     `FileMeta.ino` is 0 for memfs/FAT, so it can't key a cache today)?

In the meantime I am **not** starting C; I'm working other unblocked tasks.

**Where it bites.** `kernel/src/syscall/linux.rs` (`linux_file_mmap`,
`unmap_user_range`, `linux_file_mmap_rollback`); `kernel/src/mm/vma.rs`
(`VmaKind`); `kernel/src/proc/pcb.rs` (`remove_vma_range`, `try_resolve_fault`,
the VMA list + a new backing table, fork clone, teardown); `kernel/src/fs/vfs.rs`
(`FileMeta.ino` / a new stable file-identity); `kernel/src/fs/cache.rs` (the
block buffer cache C would relate to).

**Status:** PARTIAL — option **B (demand-paged `MAP_PRIVATE`)** built
autonomously 2026-06-14 (TD22 Phase 1). Option **C (unified page cache +
writable `MAP_SHARED`)** remains **OPEN** for the operator.

---

## Q6 — Cross-process memory introspection (`process_vm_readv`/`writev`, `ptrace`): permit it at all, and behind what gate? — OPEN (2026-06-14)

**Background.** `process_vm_readv(2)` / `process_vm_writev(2)`
(`process_vm_impl` in `kernel/src/syscall/linux.rs`) currently implement the
**same-address-space** transfer (the target thread shares the caller's PCB) but
return **`-ESRCH`** for any *cross-process* target — explicitly documented in the
code as "Cross-AS not implemented." Likewise `sys_ptrace` returns **`-EPERM`
unconditionally** (no tracer may ever attach). So today the kernel permits **no
cross-process memory introspection of any kind** — a coherent, deliberate
security posture.

The 2026-06-14 zero-copy work added the missing *mechanism*: pml4-parameterized
`copy_from_user_as` / `copy_to_user_as` (`kernel/src/mm/user.rs`) can read/write
an arbitrary address space's user pages through the HHDM. Wiring the cross-process
data path in `process_vm_impl` is now mechanically straightforward (resolve the
target's pml4 via `pcb::get_pml4`, route the *remote* side of each copy through
the `_as` primitive while the *local* side stays on the current CR3). **The only
thing missing is the authorization model** — and that is a genuine design fork,
not something to default my way through.

**Question.** Should cross-process `process_vm_readv`/`writev` (and, relatedly,
real `ptrace` attach) be allowed at all — and if so, gated by what?

**Options.**

- **A. Keep the status quo: no cross-process introspection (`ESRCH`/`EPERM`).**
  - *Pros:* maximally safe; consistent with the current posture; nothing to
    design; gdb/strace/CRIU simply can't peer into *other* processes (they still
    work on the same-AS / self case).
  - *Cons:* real debuggers (`gdb attach`, `strace -p`, `lldb`), checkpoint/restore
    (CRIU), and some profilers genuinely need cross-process reads; they'll fail.

- **B. Allow it, gated by a capability the caller must hold over the target.**
  Consistent with the design spec's "capability-based security from day one, no
  ambient authority": cross-process memory access requires the caller to hold an
  unforgeable handle/capability to the target process (e.g. a `ProcessCap` with a
  DEBUG/INTROSPECT right), not merely to know its PID. A debugger would obtain
  that capability through an explicit grant (parent→child, or a privileged broker).
  - *Pros:* aligns with the microkernel capability model; far stronger than
    Linux's PID-plus-yama check; auditable; no ambient authority.
  - *Cons:* requires designing the process-capability + right (does one exist
    yet?), a grant path, and plumbing it through `process_vm_impl` and `ptrace`;
    debuggers must be taught to acquire the capability (not a drop-in Linux ABI).

- **C. Allow it, gated by a Linux-style `ptrace_may_access` (same-uid / CAP_SYS_PTRACE / yama).**
  - *Pros:* drop-in compatible with how Linux debuggers expect to work; familiar.
  - *Cons:* "ambient authority by PID + uid" is exactly what the design spec says
    to avoid; requires a real uid/cred model and a yama-scope policy knob; weaker
    than B.

**Claude's recommendation.** Defer — this is a security-policy fork the operator
should own. If forced to pick a default I'd lean **A (keep ESRCH/EPERM)** until
there's a concrete consumer, because it's safe and the mechanism can be wired in
a day once the gate is decided; and **B** as the eventual target since it matches
the capability-based design. I am **not** opening cross-process access
autonomously — silently granting any process the ability to read/write any other
process's memory would be a serious regression and contradicts the core design.

**Where it bites.** `kernel/src/syscall/linux.rs` (`process_vm_impl` — the
`if !same_addr_space { return ESRCH }` arm; `sys_ptrace`); `kernel/src/mm/user.rs`
(`copy_from_user_as`/`copy_to_user_as` — the mechanism, already built);
`kernel/src/proc/pcb.rs` (`get_pml4`); and whatever process-capability type a
choice of **B** would introduce (`kernel/src/cap/`).

**Status:** OPEN — mechanism exists; authorization model is the operator's call.
