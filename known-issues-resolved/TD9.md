### TD9. Linux program interpreter (ld.so) + PIE executable loaded at a fixed base — no ASLR — RESOLVED 2026-06-14

**Resolution (PIE-executable base, 2026-06-14):** the main `ET_DYN`/PIE
executable base is now randomised too. A new `choose_exec_load_bias(is_pie)`
helper (`kernel/src/proc/spawn.rs`) returns `0` for `ET_EXEC` and, for PIE,
an ASLR base ≥ `LINUX_PIE_BASE` drawn via `apply_aslr_base(LINUX_PIE_BASE,
rng::next_bounded(PIE_ASLR_SPAN_PAGES))` (28 bits of entropy, 16 KiB-page
units, falling back to the fixed floor before the CSPRNG is seeded). It is
computed once per spawn/exec at the two `exec_load_bias` sites
(`spawn_process` + `exec_process`) and already threads uniformly through
`load_segments_with_bias`, the biased entry point, and the SysV stack
builder's `AT_ENTRY`/`AT_PHDR`, so the whole image relocates consistently.
The highest PIE base (`≈0x5955_5555_0000`) leaves ~22 TiB below the
interpreter floor (`0x7000_0000_0000`) for the image + brk growth, and the
PIE floor sits far above the mmap window (`0x60_0000_0000`), so no
collision is possible. `sys_brk` is now a real demand-paged heap (see the
"Linux brk(2) heap" resolution below): a PIE image's heap grows from its
page-aligned image end up to a ceiling of `LINUX_INTERP_BASE`, i.e. into
that 22 TiB headroom, and the grow path's VMA-overlap check is a second
guard against colliding with the interpreter or mmap window. Covered by
`spawn::self_test`'s
`test_pie_aslr_window` (alignment + ≥1 TiB interpreter-floor headroom).
Both halves of TD9 are now done; entropy/always-on policy is in
design-decisions.md #20.

**Resolution (interpreter base, 2026-06-14):** `load_interpreter` in
`kernel/src/proc/spawn.rs` now draws a per-exec randomised base from the
`LINUX_INTERP_BASE` window instead of using the fixed constant. A new pure
helper `apply_aslr_base(fixed_base, rand_pages)` adds `rand_pages *
FRAME_SIZE` (saturating) to the low edge; the page index is drawn unbiased
from `[0, 2^INTERP_ASLR_BITS)` via `rng::next_bounded`. `INTERP_ASLR_BITS =
28` mirrors Linux x86_64's default `mmap_rnd_bits` (28 bits of layout
entropy), applied in our 16 KiB page units → a 4 TiB window whose top
(`≈0x73FF_FFFF_C000`) stays far below `USER_STACK_GUARD`, so a randomised
base can never collide with the stack, the low-loaded executable, the brk
heap, or the general mmap window (`0x0060_…`); the interpreter image is the
window's sole occupant, so intra-window collisions are impossible too.
`AT_BASE` already carried whatever base was chosen, so ld.so relocation is
unaffected. Before the CSPRNG is seeded (very early boot, before any Linux
process can spawn in practice) it falls back to the fixed low edge.
Covered by `spawn::self_test`'s `test_apply_aslr_base` (alignment +
in-window + stack-clearance + saturation) and the existing
`self_test_linux_dynamic_interp` end-to-end launch (the test interpreter's
exit code is register-only/position-independent, so it runs correctly at
any randomised base; verified loading at e.g. 0x701e77808000, not the fixed
0x700000000000). The entropy-bits choice is recorded in
design-decisions.md.

---



**Resolution (interpreter base, 2026-06-14):** `load_interpreter` in
`kernel/src/proc/spawn.rs` now draws a per-exec randomised base from the
`LINUX_INTERP_BASE` window instead of using the fixed constant. A new pure
helper `apply_aslr_base(fixed_base, rand_pages)` adds `rand_pages *
FRAME_SIZE` (saturating) to the low edge; the page index is drawn unbiased
from `[0, 2^INTERP_ASLR_BITS)` via `rng::next_bounded`. `INTERP_ASLR_BITS =
28` mirrors Linux x86_64's default `mmap_rnd_bits` (28 bits of layout
entropy), applied in our 16 KiB page units → a 4 TiB window whose top
(`≈0x73FF_FFFF_C000`) stays far below `USER_STACK_GUARD`, so a randomised
base can never collide with the stack, the low-loaded executable, the brk
heap, or the general mmap window (`0x0060_…`); the interpreter image is the
window's sole occupant, so intra-window collisions are impossible too.
`AT_BASE` already carried whatever base was chosen, so ld.so relocation is
unaffected. Before the CSPRNG is seeded (very early boot, before any Linux
process can spawn in practice) it falls back to the fixed low edge.
Covered by `spawn::self_test`'s `test_apply_aslr_base` (alignment +
in-window + stack-clearance + saturation) and the existing
`self_test_linux_dynamic_interp` end-to-end launch (the test interpreter's
exit code is register-only/position-independent, so it runs correctly at
any randomised base). The entropy-bits choice is recorded in
design-decisions.md.

**What remains (PIE-executable base — still DEBT):** the position-independent
*main* executable is still loaded at the fixed `LINUX_PIE_BASE =
0x5555_5555_4000`. Randomising it is more delicate than the interpreter
because the brk heap grows immediately above the PIE image, so the PIE
ASLR window must be chosen to leave room for brk growth without colliding
with the mmap window below or the interpreter window above. Deferred as a
separate follow-up. Original debt write-up follows.

---



**What:** The Linux dynamic-linker load path (`load_interpreter` in
`kernel/src/proc/spawn.rs`) maps the program interpreter (ld.so) at a
**fixed** virtual base, `LINUX_INTERP_BASE = 0x0000_7000_0000_0000`,
every time.  Real Linux randomises the interpreter base (and the mmap
region generally) via ASLR.  The executable itself is also loaded at its
fixed link-time vaddr (PIE executables are not yet re-based either).

**Where:** `kernel/src/proc/spawn.rs` — the `LINUX_INTERP_BASE` constant
and `load_interpreter()`.  AT_BASE is reported correctly from whatever
base is chosen, so making this random is a localised change.

**Why it's debt, not a bug:** ASLR is a security hardening measure, not a
correctness requirement — ld.so relocates itself to wherever it is placed
using the base it is told (AT_BASE) and its own dynamic relocations.  A
fixed base is fully functional; it just removes the address-space
randomisation defence against exploitation.

**Proper fix:** Once a userspace mmap-region allocator / ASLR policy
exists, draw the interpreter base (and PIE executable base) from it with
per-exec randomisation instead of the fixed constant.  Keep the AT_BASE
plumbing as-is — it already carries whatever base is chosen.

**Update 2026-06-14:** the dependency is now in place — a per-process
VMA-aware mmap gap allocator (`pcb::reserve_unmapped_area` →
`mm::vma::find_gap`, fronted by `handlers::alloc_user_mmap_reserve`) now
serves the general user mmap window with freed-gap reuse and atomic
find+insert.  ld.so's general-region maps already flow through it; what
remains for TD9 is purely the *randomisation policy*: pick a randomised
base for the interpreter/PIE load instead of the fixed `LINUX_INTERP_BASE`
constant.  Note the interpreter is loaded at `0x7000_…`, disjoint from the
mmap window `0x0060_…`, so ASLR for it will need its own randomised
placement (or be folded into the mmap region) rather than just calling the
new allocator.

**Related limitation (not debt, just unimplemented):** end-to-end
interpreter *execution* is untested because no real glibc/musl ld.so is
on the filesystem yet.  The load mechanism (base selection, biased
segment mapping via `load_segments_with_bias`, AT_BASE/AT_ENTRY auxv) is
unit-tested via `spawn::test_load_interpreter_fallbacks` (static-ELF and
absent-interpreter `Ok(None)` fallbacks).  See `todo.txt` "Linux
dynamic-linker (ld.so) load path".
