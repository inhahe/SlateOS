## 25. Path Z libc + rootfs — go straight to **glibc** on an **ext4** rootfs (no musl stepping-stone)

**Date:** 2026-06-14

**Decided by:** Claude (operator-delegated). The operator left the call to Claude
(Q8) with an explicit standing preference: *"I prefer doing all the hard work
upfront over doing extra labor just to make to reach a milestone quicker by doing
scaffolding … whichever one you think will be more efficient in the long run."*
Claude had previously *recommended* the musl-first staged approach (option C in
`open-questions.md` Q8); on reconsideration against the operator's preference and
the current state of the loader, Claude reversed that recommendation and chose
glibc-direct. This is Claude's to revisit; the operator may overrule.

**Context:**
Path Z (run *prebuilt* Linux toolchain binaries on the Linux-ABI layer — Q4,
operator-prioritized) is fully built and proven for **static** binaries
end-to-end (`proc::spawn::self_test_linux_file_mmap`, `self_test_linux_brk` spawn
real ring-3 Linux-ABI processes). The one documented blocker for **dynamic**
execution (`roadmap.md` line 5089) is that there is no real libc + `ld.so` on a
real on-disk filesystem yet. Choosing the libc steers a large amount of
subsequent ABI-compat work and is costly to reverse, so it was deferred to the
operator (Q8) and delegated back to Claude.

**Decision:**
- **libc: glibc.** Bring up the dynamic-execution path directly against glibc
  (`ld-linux-x86-64.so.2` + `libc.so.6` + friends) — the libc the prioritized
  prebuilt distro toolchain (GCC/binutils/CMake/Make, Q3) is actually linked
  against. No intermediate musl bring-up.
- **rootfs: ext4.** Populate a real ext4 image with the libc tree, per the
  design's *"ext4 first"* rule, replacing the current FAT-only test image as the
  vehicle for the Linux-ABI root. (FAT image stays for the FAT driver self-test.)

**Rationale:**
- The operator's standing preference is to do the hard work upfront and avoid
  throwaway scaffolding. musl-static-first is precisely "extra labor to reach a
  milestone quicker via scaffolding": it would require building/sourcing a musl
  rootfs and debugging musl-specific `ld-musl`/ABI quirks that are then discarded
  for the real glibc target.
- The de-risking value that originally motivated the musl-first recommendation
  has largely been *spent*: the static-load path is already validated end-to-end,
  so the only incremental thing musl-*dynamic* would prove cheaply is the
  dynamic-linker machinery in isolation. That benefit is real but modest, and it
  does not transfer to glibc (glibc still needs its own ABI-surface debugging).
- The shared infrastructure (ELF dynamic loading, relocation processing, TLS
  setup, `ld.so` invocation) must be built regardless and is the bulk of the
  work; building it directly against the real target avoids a duplicated rootfs
  setup with no proportional payoff.
- Net: glibc-direct matches the operator's preference and, given the static path
  is already proven, is at worst a wash on long-run efficiency while saving a
  full second rootfs/ABI bring-up.

**Alternatives considered:**
- **musl static-first, then glibc (Claude's original Q8 recommendation, option
  C).** Cheapest path to *a* real compiled binary and isolates dynamic-linker
  bugs from glibc's large ABI surface — but duplicates rootfs setup, adds
  throwaway musl-specific debugging, and its de-risk value is small now that the
  static path is proven. Rejected per operator preference + small marginal value.
- **musl only.** Rejected: most prebuilt distro toolchain binaries (the actual
  Path-Z target) are glibc-linked, so musl proves the loader but never runs the
  prioritized binaries.

**Risk accepted:** glibc is a much larger first-light bring-up (TLS edge cases,
`__libc_start_main`, vDSO, NSS, locale, many more syscalls/`ioctl`s) hit all at
once with no musl intermediate. If glibc cold-bring-up proves to be a long,
hard-to-bisect debug cycle, a *minimal* musl-static smoke test remains available
as a fallback diagnostic to isolate dynamic-linker bugs from glibc-ABI gaps —
this decision does not preclude that, it only declines to make musl the planned
first milestone.

**Where it lives:** `scripts/create-disk.py` (rootfs build — currently FAT test
image only; needs an ext4 image populated with the glibc tree),
`kernel/src/proc/spawn.rs` (`load_interpreter`, the `ld.so` entry path),
`kernel/src/elf.rs` (`interp_path`/`load_segments_with_bias`),
`kernel/src/syscall/linux.rs` (further ABI gaps glibc will exercise), and the
ext4 mount/root path. `roadmap.md` line 5089.

**How to reverse:** the libc/rootfs choice is localized to the rootfs builder and
the on-disk libc tree; switching to musl would mean swapping the `ld-musl`/libc
files into the rootfs and chasing musl-specific ABI quirks. The loader plumbing
itself is libc-agnostic, so reversal cost is dominated by rootfs rebuild +
re-validation, not kernel code.
