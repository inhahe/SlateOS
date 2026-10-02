### FIXED-A-KSYMS-COULD-NEVER-HAVE-LOADED-THIS-KERNELS-SYMBOLS (lane A)

**Status:** FIXED 2026-08-23 (commits `98fec0c94`, `0b43b6443`, `4143cf341`) —
awaiting a boot test on `main`.

**In short:** When the kernel crashes it prints a "backtrace" — the list of
functions it was inside when it died. For an unknown length of time that list
printed only raw numbers, with no function names, making every crash report
far harder to act on. Two separate faults caused it, and the second was hidden
behind the first. Both are fixed; backtraces name functions again.

**Where:** `scripts/boot-test.sh` (the strip step) and `kernel/src/ksyms.rs`.

**Fault 1 — the boot image had no symbol table at all.** `boot-test.sh` staged
the kernel through `llvm-strip` with no flags. A bare strip removes `.symtab`,
which is the only thing `ksyms` reads. Every boot logged

```
[ksyms] No symbol table found in kernel ELF
```

at serial line ~131 and carried on, so every panic backtrace in the project
printed bare addresses. Fixed by using `--strip-debug`, which removes the
DWARF debug info (the bulk: 215 MiB → 58 MiB, still 73% off) but keeps
`.symtab`.

**Fault 2 — revealed by fixing fault 1.** With `.symtab` present, `ksyms`
reached its own parse for the first time in a long while and panicked boot
batch 28 immediately:

```
memory allocation of 20160000 bytes failed
```

`parse_elf_symbols` sized both its buffers from `sym_count`, the *total*
`.symtab` entry count:

```rust
let mut entries = Vec::with_capacity(sym_count / 2); // Roughly half are functions.
let mut names   = Vec::with_capacity(sym_count * 20); // ~20 chars per name average.
```

This kernel's `.symtab` holds ~1,008,000 entries, dominated by the SECTION,
FILE and NOTYPE entries every object file contributes; only ~12% (~126,000)
survive the FUNC/OBJECT filter. So `names` reserved 19.2 MiB and `entries` a
further 8 MiB, for data needing ~2 MiB.

**The 19.2 MiB request could not have succeeded at any point in boot, with any
amount of memory free.** `mm::heap` serves anything too big for a slab
straight from the buddy allocator, and `MAX_ORDER` of 10 caps a single
allocation at 2^10 × 16 KiB = **16 MiB**. 19.2 MiB rounds to order 11 and is
refused on arithmetic, not on availability — the frame allocator had 2.9 GiB
free at the moment of the panic. **Any future change here that reintroduces a
single multi-megabyte allocation must respect that 16 MiB ceiling.**
*(2026-09-25: the ceiling is gone -- heap allocations above 16 MiB are mapped
from vmalloc, design-decisions.md §959 -- so such a request would now succeed.
The lesson that survives is the other one: size buffers from a count, not a
guess.)*

**The fix** removes the copy rather than resizing it. `name_offset` now indexes
the kernel ELF's own `.strtab`, borrowed in place, so name data costs nothing.
This is sound because Limine's kernel-file mapping is permanent: `mm::frame`
seeds its free lists from `USABLE` regions only (lines 1765/1794), so the
`BOOTLOADER_RECLAIMABLE` region holding the kernel file is never handed out
and never reused. `entries` is now sized by a counting pass rather than a
guess.

**The lesson, and why it went unnoticed.** The module doc asserted "a typical
kernel has ~2000-5000 functions … total ~100-200 KiB". It has ~124,000 — a
25-60× error in a comment that read as settled fact, sitting directly above
the code whose constants derived from it. Nothing re-derived it as the kernel
grew two orders of magnitude, and fault 1 meant the code never ran to
contradict it. **A stale capacity estimate in a doc comment is a latent bug
with a plausible alibi.**

Related: the same investigation added `ksyms::resolve_static` (lock-free,
allocation-free) so panic/spin-stall/lockdep reporters can resolve symbols
without re-entering the machinery they are diagnosing. See commit
`0b43b6443`.
