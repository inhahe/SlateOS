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

## Q16 `container diff` baseline semantics — OPEN

- **Question** — What should Docker-style `container diff` (list filesystem
  changes since the container's "base") compare against, given our container
  model supports two rootfs kinds?
- **Context** — Two kinds of container rootfs exist today:
  1. **Overlay-backed** (created via `oci run`): a real overlayfs with a
     read-only lower (the image layers) and a writable upper. Docker's diff is
     *defined* here — Added/Changed = entries in the upper, Deleted = whiteouts.
     `kernel/src/fs/overlay.rs` already tracks `whiteouts` and the upper dir, so
     this is computable cleanly **iff** the container records its `OverlayId`.
  2. **Plain bind-rootfs** (created via `container create` + `container rootfs
     <dir>`): a chroot to a plain host directory with no lower/upper distinction.
     There is no natural "base" to diff against.
- **Options**
  - **A. Overlay-only diff.** Implement `diff` only for overlay-backed
    containers (enumerate upper + whiteouts, classify A/C/D); return
    `NotSupported` for plain bind-rootfs containers. *Pro:* matches Docker
    semantics exactly, no band-aid, cheap (no rootfs walk). *Con:* needs the
    container to record its `OverlayId` (today it only stores `rootfs_mount`, a
    path); `diff` is unavailable for the common plain-rootfs path.
  - **B. Point-in-time baseline.** Capture a manifest (path → size/mtime, or a
    content hash) of the rootfs when the container first `start()`s, and diff the
    live tree against it. *Pro:* works for every container regardless of rootfs
    kind. *Con:* not Docker's semantics (baseline is "first start", not "image"),
    adds a full rootfs walk + a stored per-container manifest on the start hot
    path, and a large rootfs makes start() expensive.
  - **C. Both.** Overlay diff when an overlay is present, fall back to a
    point-in-time baseline otherwise. *Pro:* always available, exact where it can
    be. *Con:* two code paths and two different meanings of "diff" under one
    command — potentially confusing.
- **Claude's recommendation** — **A** (overlay-only), as the only option that is
  a *proper* (non-band-aid) implementation matching Docker. It requires a small
  plumbing change: record the `OverlayId` on the `Container` struct at
  `oci run` time. In the meantime `container diff` is simply not implemented;
  all other `container` subcommands (export/import/commit/prune/rm -f/…) are
  done and don't depend on this.
- **Where it bites** — `kernel/src/container.rs` (`Container` struct would gain
  an `overlay_id: Option<OverlayId>` field set on the `oci run` path; a new
  `diff(id)` fn), `kernel/src/fs/overlay.rs` (would need an `upper_entries(id)`
  enumerator + expose `whiteouts`), `kernel/src/oci.rs` (overlay creation site),
  `kernel/src/kshell.rs` (`container diff` arm).
- **Status** — `OPEN` (deferred; not blocking — other container increments
  continue).

---

All deferred operator decisions (Q1–Q15) have been resolved — see the
"Recently resolved" list below and `design-decisions.md` for full rationale. New
decisions should be appended above this line as `## Q17 …`.

---

Recently resolved (see `design-decisions.md` for the full rationale):

- The coreutils "which set is canonical?" question — resolved 2026-06-12;
  standalone per-tool crates are canonical (§8).
- Q1 `set_mempolicy_home_node` / NUMA mempolicy on UMA — resolved 2026-06-13,
  **operator-confirmed 2026-06-14**; keep the UMA no-op returning 0, option A
  (§10).
- Q2 `/proc/sys/vm/overcommit_memory` & memory-commit policy — resolved
  2026-06-13, **operator-confirmed 2026-06-14** (keep the shipped defaults:
  native strict/committed, Linux lazy/overcommit; both configurable); build the
  both-strategies model (Option 5); map the system-wide overcommit knob to a
  fine-grained native cap (`admin.memory_policy`), not `CAP_SYS_ADMIN` (§11).
- Q3 next major initiative — resolved 2026-06-13; terminal/dev before GUI,
  GCC/CMake/Make toolchain first, CPython then fastpy (§9).
- Q4 toolchain on Slate OS: run-prebuilt-Linux vs native-port — resolved
  2026-06-13; **Path Z** (run prebuilt Linux toolchain binaries on the Linux-ABI
  layer now, native-port selectively later), native-first/no-leak kept
  inviolate, clang green-lit for install (§12).
- Q5 file-backed `mmap` — how far to take the fix — resolved 2026-06-14
  (§22), then **REOPENED 2026-06-14** by the operator, then **RE-RESOLVED
  2026-06-14**: adopt **C-lite** (a unified *read-only* page cache for
  shared-library text dedup + de-double-caching), deferred until a concrete
  consumer appears (the dynamic linker is the likely first; stable VFS
  file-identity is the precursor); writable `MAP_SHARED` writeback stays declined
  / `ENOSYS` (§23). Deferral trigger logged in `todo.txt`.
- Q6 cross-process memory introspection — resolved 2026-06-14: keep
  channel/shared-memory IPC for *consensual* sharing; add a
  **debug-capability-gated** cross-address-space `process_vm_readv`/`writev`
  (`Rights::DEBUG` on a `Process` capability; `EPERM` without it). `ptrace`
  remains a deferred follow-up behind the same gate (§24).
- Q8 Path Z libc + rootfs — resolved 2026-06-14, **operator-delegated to
  Claude**: go straight to **glibc** on an **ext4** rootfs, no musl
  stepping-stone (§25). Claude reversed its own earlier musl-first recommendation
  per the operator's stated preference for hard-work-upfront over throwaway
  scaffolding, given the static-load path is already proven end-to-end.
- Q7 kernel-task-stack-vs-IRQ overflow (B-DF1) — resolved 2026-06-15,
  **operator-chosen option A** (Claude recommended A): per-CPU guard-page IRQ
  stack with a manual nesting-aware switch + deferred preemption, plus the
  `cli`/`sti` recursion guard the restructuring exposed (§26). Validated:
  `http_gzip_8KiB` no longer double-faults at the gzip→dashboard transition.
- Q9 bare-ELF ABI auto-classification — resolved 2026-06-24, **operator-chosen
  option D** (Claude recommended D): default unmarked bare ELF → Linux ABI, add
  `NT_GNU_ABI_TAG` note-walk as a positive Linux signal, stamp native binaries
  with an explicit SlateOS marker; `spawn_process_with_abi` override kept (§33).
- Q10 fullscreen-capture video codec — resolved 2026-06-24, **operator deferred
  to Claude's recommendation**: hardware encode via the GPU driver long-term
  (option C), defer the software-codec port near-term (option D), no stub
  encoder meanwhile; if a software path is ever needed first, AV1/`rav1e` over
  H.264 (§34).
- Q11 zero-copy page-flipping for large channel messages — resolved 2026-06-24,
  **operator-chosen option B** (Claude recommended B): explicit opt-in
  `MSG_ZEROCOPY`-style flag + caller-provided page-aligned landing region; copy
  path stays the default. Compiler follow-up: keep it programmer/library-
  controlled (library-level auto-threshold helper), the compiler does not
  auto-insert the flag (§35).
- Q12 next large initiative — resolved 2026-06-24, **operator-chosen option E**:
  build the C-lite read-only page cache now; lifts the §23 "not now" hold (§36).
- Q13 de-double-cache file data — resolved 2026-06-30, **operator-chosen option A**
  (Claude recommended A): page-cache-primary — the page cache is the single cache
  for regular-file data, the buffer cache caches only filesystem metadata (§38).
- Q14 connect the two cgroup subsystems — resolved 2026-06-30, **operator-chosen
  option A** (Claude recommended A): cgroupfs as the frontend,
  `kernel/src/cgroup.rs` as the enforcement engine; fork/clone/spawn inherit
  `cgroup_id` (§39).
- Q15 next focus — resolved 2026-06-30, **operator-chosen option A then C/D**:
  execute Q13 + Q14 first, then a large initiative — C (GPU accel) or D (Docker /
  container-runtime port) in operator-indifferent order; this is the explicit
  go-ahead for the Docker port (§40).

---
