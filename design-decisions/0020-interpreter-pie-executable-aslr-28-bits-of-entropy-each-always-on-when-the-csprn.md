## 20. Interpreter + PIE-executable ASLR — 28 bits of entropy each, always-on when the CSPRNG is seeded (no personality opt-out yet)

**Date:** 2026-06-14 (interpreter); 2026-06-14 (PIE base)
**Decided by:** Claude (autonomous) — reversible; the operator may overrule. This
fully resolves known-issues.md TD9, whose documented "proper fix" was always to
randomise the load bases; the only genuine choices were *how much entropy* and
*whether to honour an opt-out*. Both load bases (ld.so interpreter and the PIE
main executable) are now randomised under the same policy.

**Problem:**
`load_interpreter` (`kernel/src/proc/spawn.rs`) loaded ld.so at the fixed
`LINUX_INTERP_BASE = 0x7000_0000_0000` every exec, removing the ASLR defence.
With the VMA-aware mmap allocator now in place (decision #19), the remaining
work was purely the randomisation policy. Two sub-decisions had real tradeoffs.

**Decision:**
1. **Entropy = 28 bits, in 16 KiB-page units** (`INTERP_ASLR_BITS = 28`). The
   per-exec base is `LINUX_INTERP_BASE + next_bounded(2^28) * FRAME_SIZE`
   (saturating), via the pure `apply_aslr_base` helper. 28 mirrors Linux
   x86_64's default `mmap_rnd_bits` (28) — i.e. the same *number of equally
   likely bases* (2^28), which is the security-relevant metric, even though our
   16 KiB pages make the byte-range (4 TiB) differ from Linux's (1 TiB at 4 KiB
   pages). The 4 TiB window's top (`≈0x73FF_FFFF_C000`) stays far below
   `USER_STACK_GUARD`, so collisions with the stack/executable/brk/mmap-window
   are impossible (the interpreter is the window's sole occupant). A
   `spawn::self_test` assertion guards this clearance invariant against future
   bit-count changes.
2. **Always-on when seeded; fixed-base fallback before the CSPRNG is seeded.**
   No `personality(ADDR_NO_RANDOMIZE)` / `setarch -R` opt-out yet (our
   `sys_personality` accepts but does not honour bits — see todo.txt). ASLR is
   a pure hardening win and every modern OS defaults it on, so always-on is the
   right default; a per-process opt-out can be wired through personality later
   if a debugger needs deterministic addresses.

**Alternatives considered:**
- *Match Linux's byte-range (1 TiB) by using ~26 bits.* Rejected: entropy (bit
  count), not byte span, is the ASLR security metric; matching Linux's 28-bit
  entropy is the principled choice, and our window has ample room for it.
- *Fold the interpreter into the general mmap region and let the gap allocator
  place it.* Rejected for now: the interpreter window (`0x7000_…`) is disjoint
  from the mmap window (`0x0060_…`) by design (TD9 note), and randomising
  within its own dedicated, collision-free window is simpler and lower-risk
  than threading interpreter placement through the general allocator.
- *Match Linux's byte-range for the PIE base.* Same rejection as the
  interpreter: entropy is the metric. The PIE window reuses the 28-bit policy.

**PIE-executable base (second half of TD9):**
The PIE main-executable base previously loaded at the fixed
`LINUX_PIE_BASE = 0x5555_5555_4000` (Linux's `ELF_ET_DYN_BASE`). `exec_load_bias`
is computed once per spawn/exec and threaded through `load_segments_with_bias`,
the biased entry point, and the AT_ENTRY/AT_PHDR auxv, so a single helper
suffices: `choose_exec_load_bias(is_pie)` returns `0` for ET_EXEC, and for PIE
returns `apply_aslr_base(LINUX_PIE_BASE, next_bounded(2^28))` when the CSPRNG is
seeded (fixed `LINUX_PIE_BASE` fallback otherwise) — the *same* `apply_aslr_base`
helper and 28-bit entropy (`PIE_ASLR_BITS = 28`) as the interpreter. The 4 TiB
PIE window sits far above the mmap window (`0x0060_…`) and far below the
interpreter window (`0x7000_…`), leaving ≥1 TiB of headroom below the
interpreter floor (asserted by `test_pie_aslr_window` in `spawn::self_test`). As of
2026-06-14 the brk heap is real (see entry #21 below): a PIE image's heap grows
from its page-aligned image end up to a ceiling of `LINUX_INTERP_BASE` (the
interpreter window floor), i.e. into this window's headroom, so the "brk grows
above the PIE image" concern is now handled by the `brk_ceiling` bound plus the
grow-path VMA-overlap guard.

**How to reverse:** set the bases back to the `LINUX_INTERP_BASE` /
`LINUX_PIE_BASE` constants in `load_interpreter` / `choose_exec_load_bias` (drop
the `is_initialized()`/`apply_aslr_base` blocks) and remove
`test_apply_aslr_base` / `test_pie_aslr_window`. To instead make it opt-out-able,
gate the randomisation on a per-process "no randomise" flag fed from
`personality(ADDR_NO_RANDOMIZE)`.
