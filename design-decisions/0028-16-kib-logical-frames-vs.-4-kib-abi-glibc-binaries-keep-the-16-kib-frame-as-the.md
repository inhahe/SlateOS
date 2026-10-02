## 28. 16 KiB logical frames vs. 4 KiB-ABI glibc binaries — keep the 16 KiB frame as the alloc/RSS/reclaim unit, add 4 KiB-sub-frame permission granularity for mmap/mprotect/ELF load

**Date:** 2026-06-15

**Decided by:** Claude (operator-approved scope). The operator settled the
*destination* — run prebuilt dynamically-linked **glibc** binaries on an **ext4**
rootfs, "Path Z," with no musl stepping-stone (see §25). This entry records the
specific *mechanism* I chose autonomously to get there; it's mine to revisit, but
the goal it serves is operator policy.

**Context:**
Slate uses a **16 KiB logical page/frame** (`FRAME_SIZE = 16384`,
`HW_PAGES_PER_FRAME = 4`) as the design-mandated base page (CLAUDE.md
"Architectural Rules"). But standard x86-64 Linux/glibc binaries are linked with
`max-page-size = 0x1000` (**4 KiB**). Consequently `ld.so`'s `_dl_map_segments`:
- maps adjacent ELF segments with *different* permissions (R-- rodata immediately
  followed by RW- data) on **4 KiB** boundaries that fall *inside* one 16 KiB
  frame, and
- issues `MAP_FIXED` overlays and `mprotect` calls (notably the RELRO step
  `mprotect(…, 0x4000, PROT_READ)`) at **4 KiB** alignment that is *not* 16 KiB
  aligned.

A frame-granular memory subsystem cannot represent "the first 4 KiB of this frame
is read-only, the next 4 KiB is read-write," nor honor a 4 KiB-aligned
`mprotect`/`MAP_FIXED` — which is exactly what broke real-glibc execution
(bss zero-fill overlay → "cannot map zero-fill pages"; RELRO → "cannot apply
additional memory protection"). The hardware already uses 4 KiB PTEs (our 16 KiB
frame = 4 contiguous 4 KiB PTEs), so the capability exists at the HW level; the
question was how to expose it without abandoning the 16 KiB design.

**Options considered:**
- **(A) Switch the OS base page to 4 KiB.** Trivially compatible with stock
  Linux binaries, but violates a core, deliberate architectural decision (16 KiB
  pages chosen for fewer TLB entries / smaller page tables / better large-working-
  set behavior) and would require rebuilding the *entire* memory subsystem around
  4 KiB. Rejected: throws away a foundational design choice to accommodate one
  compatibility path.
- **(B) Relink/patch every Linux binary to 16 KiB max-page-size.** Defeats the
  whole point of Path Z (running *unmodified, prebuilt* distro binaries) and is
  impossible for closed-source blobs. Rejected.
- **(C, chosen) Keep 16 KiB as the allocation/RSS/rmap/reclaim unit; add 4 KiB
  sub-frame *permission and file-backing* granularity** on the demand-fault and
  mmap/mprotect paths only. One physical 16 KiB frame still backs all 4 subpages
  and is still the unit that the allocator, RSS accounting, reverse-mapping, and
  reclaim operate on; but each of its 4 hardware PTEs may carry independent
  permissions and (for file maps) independent backing.

**Why C:**
- It preserves the 16 KiB architecture everywhere it matters for performance
  (allocation, accounting, reclaim, the common single-VMA fast path is
  byte-for-byte unchanged) while exposing exactly the 4 KiB granularity the
  hardware already has and stock binaries require.
- The added cost is paid **only** on the slow paths that actually need it: a
  fault on a frame straddled by >1 VMA, a 4 KiB-granular `mmap(MAP_FIXED)`, or a
  4 KiB-granular `mprotect`. A fault on a frame covered by a single VMA takes the
  original fast path.
- It is the minimal change that makes unmodified glibc work — no other
  subsystem's invariants change.

**Mechanism (where it lives):**
1. **Per-subpage demand faulting** — `pcb::resolve_subpaged_fault` (routed to from
   `pcb::try_resolve_fault` when a faulting frame is straddled by more than one
   VMA): allocates/zeroes one 16 KiB frame, then for each 4 KiB subpage installs a
   PTE with that subpage's covering-VMA permissions and file backing via
   `page_table::map_4k_if_absent`. RSS/rmap/reclaim still key on the 16 KiB base.
2. **4 KiB page-table primitives** — `page_table::change_flags_4k` (flip one leaf
   PTE), `map_4k_if_absent` / `unmap_4k`, `is_hw_page_aligned`, `HW_PAGE_SIZE` /
   `HW_PAGES_PER_FRAME`.
3. **4 KiB-granular anonymous `MAP_FIXED`** — `linux_anon_mmap_fixed` + the
   `sys_mmap` fixed-dispatch path (4 KiB align/round, net RLIMIT_AS charge,
   `unmap_user_range` + `remove_vma_range` of the replaced range, Anonymous VMA).
4. **4 KiB-granular `mprotect`** — `mprotect_validate_args` / `sys_mprotect` gate
   and step on `HW_PAGE_SIZE` and flip individual PTEs via `change_flags_4k`.
5. **Per-subpage ELF segment loading** — `proc/elf.rs` two-pass loader: pass 1
   computes the 16 KiB-frame-aligned span over all biased PT_LOAD; pass 2 maps
   each 16 KiB frame with the **union** of its overlapping segments' permissions
   (preserving W^X), copies the overlapping file bytes, and maps via
   `map_frame_subpages`.

**Validation:** `proc::spawn::self_test_linux_real_glibc` drives a real prebuilt
dynamically-linked glibc `/bin/hello` through the complete ring-3 startup —
`ld.so` maps `libc.so.6`, relocates, sets up TLS, `__libc_start_main → main →
exit_group(42)` — and the boot test reports
`REAL glibc dynamic execution … __libc_start_main → main → exit(42)): OK`
(three BOOT_OK cycles).

**Known limitation (tracked, known-issues.md TD27):** `mprotect` updates PTE
permissions but not the underlying `Vma.flags`, so a page reclaimed under memory
pressure and re-faulted is rebuilt from the *VMA's* (pre-mprotect) permissions —
e.g. a RELRO'd page would come back writable. Benign today (no reclaim path
targets RELRO pages, no swap), becomes live with anonymous swap/general reclaim;
proper fix is per-subpage VMA splitting on `mprotect`.

**How to reverse:** the sub-frame paths are additive — the fast paths and 16 KiB
primitives are untouched — so reverting means dropping the `resolve_subpaged_fault`
routing, `change_flags_4k`/`linux_anon_mmap_fixed`, the 4 KiB `mprotect` stepping,
and the per-subpage ELF loader. That would re-break unmodified glibc, so reversal
should only accompany a different compatibility strategy (A or B).
