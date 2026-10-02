## 21. Linux `brk`/`sbrk` heap — image-dependent ceiling, committed RLIMIT_AS charge, no `arch_randomize_brk` gap yet

**Date:** 2026-06-14
**Decided by:** Claude (autonomous) — reversible; the operator may overrule. The
core task (replace the `sys_brk` no-op stub with a real heap) had no genuine
fork — the stub was a latent ring-3 SIGSEGV (it claimed a grow succeeded while
mapping nothing, so glibc's malloc brk fast path would fault on first heap
write). Three sub-decisions had real tradeoffs.

**Problem:**
`sys_brk` (`kernel/src/syscall/linux.rs`) echoed the requested break and mapped
no memory. A real heap needs a heap floor/break in the PCB, a demand-paged VMA,
a growth ceiling that can't collide with other regions, and a resource-accounting
policy.

**Decision:**
1. **Image-dependent ceiling (`brk_ceiling`).** A low-loaded ET_EXEC heap
   (`brk_start < USER_MMAP_BASE`) is capped at `USER_MMAP_BASE` (the mmap window
   floor); a high-loaded PIE heap (`brk_start >= USER_MMAP_BASE`, since
   `LINUX_PIE_BASE ≈ 93 TiB` sits above the 384 GiB mmap window) is capped at
   `LINUX_INTERP_BASE` (the interpreter window floor). This is a coarse but
   always-safe bound — the heap can never grow into the mmap region, the
   interpreter, or the stack — backed by a per-grow `linux_vma_overlap_bytes`
   check as a second guard. RLIMIT_DATA bounds the heap far below this in
   practice.
2. **Committed RLIMIT_AS charge for the full grown virtual span up-front**, even
   though frames are demand-paged. This matches the project's "committed memory
   by default, no silent overcommit" design principle (CLAUDE.md / design.txt):
   a successful `brk` grow reserves the address space against RLIMIT_AS
   immediately; shrink refunds it. The alternative (charge per faulted frame)
   would be overcommit and is rejected by the design spec.
3. **`arch_randomize_brk` gap — 13 bits of entropy (added 2026-06-14).** The
   heap floor is the page-aligned image end shifted up by a random gap, mirroring
   Linux x86_64's `arch_randomize_brk` (`randomize_page(mm->brk, 0x02000000)` =
   8192 = 2^13 distinct positions at 4 KiB pages). Per the entropy-is-the-metric
   principle of decision #20, we match Linux's **13 bits** rather than its 32 MiB
   byte span; at our 16 KiB pages that is a 128 MiB max gap. Implemented as
   `spawn::choose_brk_start(image_end)` reusing the same pure `apply_aslr_base`
   helper as the load bases, always-on when the CSPRNG is seeded with an
   `image_end`-no-gap fallback before seeding (and `image_end == 0` "no heap"
   preserved exactly). The gap is dwarfed by the smallest heap window (a
   low-loaded ET_EXEC has hundreds of GiB up to `USER_MMAP_BASE`), so it never
   meaningfully reduces brk growth room or pushes the floor across `brk_ceiling`.
   Covered by `spawn::self_test`'s `test_brk_aslr_gap` (alignment + in-window over
   ET_EXEC/test/PIE bases) and exercised end-to-end by the ring-3
   `self_test_linux_brk` (which grows/writes/reads against the randomized floor).

**Alternatives considered:**
- *A single fixed ceiling for all images.* Rejected: ET_EXEC and PIE images sit
  on opposite sides of the mmap window, so one constant can't bound both without
  either forbidding ET_EXEC heap growth or letting a PIE heap grow into the mmap
  window. The image-dependent split is the minimal correct rule.
- *Per-faulted-frame RLIMIT_AS accounting (lazy charge).* Rejected: that is
  overcommit, which the design spec forbids. Up-front committed charging is the
  principled choice here.
- *Match Linux's 32 MiB byte span for the brk gap (→ 11 bits at 16 KiB pages).*
  Rejected for the same reason as the load bases: entropy (position count), not
  byte span, is the ASLR metric, so 13 bits is the principled match.

**Tests:** `syscall::linux::self_test_brk_logic` (pure: `brk_round_up`
boundary/overflow, `brk_ceiling` ET_EXEC/PIE/ordering) and the ring-3
`proc::spawn::self_test_linux_brk` (real Linux-ABI process queries its break,
grows 32 KiB, writes a sentinel into the *second* heap frame, reads it back,
exits with it — proving `set_brk_region` at load, the grow path, and
demand-paging of multiple new heap frames).

**How to reverse:** the heap is opt-in per process via `brk_start` — setting it
to 0 (as native images do) makes `sys_brk` a permanent "cannot extend" that
returns the unchanged break, so reverting to stub-like behaviour is a one-line
change at the `set_brk_region` call sites. To change the accounting policy, swap
the `linux_as_charge(added)` call for a per-fault charge in the `VmaKind::Brk`
fault resolver. To disable the randomisation gap, replace the `choose_brk_start`
calls with the bare `image_end` (and drop `BRK_ASLR_BITS`/`choose_brk_start`/
`test_brk_aslr_gap`); to make it opt-out-able, gate it on the same per-process
"no randomise" flag as the load-base ASLR.
