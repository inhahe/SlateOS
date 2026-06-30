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

## Q13 — De-double-cache the read-only page cache against the block buffer cache (§36 sub-task 4 tail)

**Status:** OPEN

**Background.** The §36 C-lite read-only page cache is functionally complete:
file identity, the `mm::page_cache` store (refcount model §37), fault-path
integration (shared RO frames + CoW on private write), VFS coherence
invalidation, and a memory-pressure shrinker — all boot-verified (two clean
BOOT_OK boots; the shrinker fired under real critical pressure, freeing 54 idle
frames with no fault). The one remaining §36 item is **performance, not
correctness**: a file's data can currently live in memory *twice*.

**Question.** How should file *data* I/O be cached so a page lives in exactly
one place? Today `mm::page_cache::get_or_fill` fills a 16 KiB page via
`fs::handle::read_at` → VFS → (for ext4/FAT) the **block buffer cache**
(`fs/cache.rs`, 512 B sectors). So an mmap'd file page is cached as 32 sectors
in the buffer cache *and* as one 16 KiB page in the page cache.

**Options.**
- **(A) Page-cache-primary (Linux-like).** Make the page cache the single cache
  for regular-file data; the buffer cache caches only filesystem *metadata*
  (superblock, bitmaps, inode tables, directory blocks). File `read`/`write`
  and mmap all go through the page cache. *Pro:* the canonical, proven design;
  truly one copy; unifies `read(2)` and mmap coherence for free. *Con:* large
  refactor of the ext4/FAT data read/write paths; must route metadata vs. data
  correctly per filesystem; biggest blast radius.
- **(B) Read-through + drop-behind.** Keep the buffer cache as the device cache,
  but have the page-cache fill path mark the sectors it consumed as
  immediately-evictable (or bypass the buffer cache for whole-page file reads),
  so the data isn't pinned in both. *Pro:* small, localized; no FS-path
  refactor. *Con:* doesn't truly unify — a concurrent `read(2)` still
  re-populates the buffer cache; coherence between `read(2)` and mmap still
  relies on the §36 invalidation hooks, not a shared frame.
- **(C) Leave as-is (status quo).** Accept the double-caching; the page cache is
  small relative to the buffer cache and the win is bounded. *Pro:* zero risk,
  ships now. *Con:* memory wasted on hot mmap'd files; not the §36 end-state.

**Claude's recommendation.** (A) is the correct long-term end-state, but it is a
real FS-data-path refactor with genuine tradeoffs, so it deserves an operator
call before I commit to it (it changes how every filesystem reads file data). If
the operator wants an incremental win first, (B) is a safe stepping stone that
(A) can later subsume. Meanwhile I am treating §36 as *delivered* (correctness +
eviction) and moving on to the next unblocked roadmap task; this optimization is
not gating anything.

**Where it bites.** `kernel/src/mm/page_cache.rs` (`get_or_fill` fill path),
`kernel/src/fs/cache.rs` (buffer cache), `kernel/src/fs/handle.rs` /
`kernel/src/fs/vfs.rs` (`read_at` routing), and the ext4/FAT data read/write
paths under `kernel/src/fs/` and `fs/`.

---

All earlier deferred operator decisions (Q1–Q12) have been resolved — see the
"Recently resolved" list below and `design-decisions.md` for full rationale. New
decisions should be appended above this line as `## Q14 …`.

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

---
