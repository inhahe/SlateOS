## TD-A-SYMBOLIZE-PY-RESOLVES-CODE-ADDRESSES-TO-DATA-SYMBOLS (lane A, 2026-08-21) — FIXED 2026-08-22

**In short:** `scripts/symbolize.py` is the tool for turning the hex addresses
in a kernel panic back into function names. Given a valid return address it
sometimes answers with the name of a *variable* instead of a function — e.g.
`kernel::KERNEL_BOOT_STACK+0xcc323 [b]` for an address that is plainly inside
`.text`. The answer is confidently wrong rather than absent, which is worse: it
sends whoever is reading a crash down the wrong subsystem.

**Where:** `scripts/symbolize.py`. Observed while symbolizing the panic in
`B-VIRTIO-GPU-FLAT-SCANOUT-WILD-WRITE` against the matching unstripped ELF.
Two of thirteen frames came back as `.bss`/`.rodata` symbols
(`KERNEL_BOOT_STACK`, `font::FONT_DATA`) with offsets in the hundreds of KiB.

**Cause:** it appears to take the nearest preceding symbol of *any* type rather
than filtering to `t`/`T` (text). A `.bss` object that happens to sit at a
lower address than the queried text address then wins the nearest-preceding
search.

**Proper fix:** filter the symbol table to `t`/`T` before the bisect, and
reject a hit whose offset lies past the symbol's recorded size (`llvm-nm
--print-size` provides it) rather than reporting it anyway. Printing
`?? (no symbol covers this address)` is a correct answer; naming a variable is
not. A ~40-line replacement doing exactly this was written ad hoc during the
diagnosis above and produced a coherent 13-frame stack where `symbolize.py` did
not; it was deleted as scratch rather than left as a second tool, because the
right outcome is one working symbolizer.

**Also worth knowing:** `llvm-symbolizer` is **not** present in the
`nightly-x86_64-pc-windows-gnu` toolchain's `bin/`, so the obvious fallback is
not available. And `awk`'s `strtonum` silently yields nothing for 64-bit kernel
addresses (they exceed double precision) — use Python for any address
arithmetic in these scripts.

### Fixed 2026-08-22 — and the diagnosis above was half wrong

`scripts/symbolize.py` now takes extents from `nm --print-size` instead of the
gap to the next symbol, searches text symbols before the full table, and ships
a `--self-test` that derives its cases from the ELF at run time.

**The reproduced defect was the *size* one, not the *kind* one.** Against the
current debug ELF, `0xffffffff8254ea30` resolved to
`drm::ati::KNOWN_DEVICES+0x3c28` — a **sixteen-byte** array claiming an address
15 KiB past its own end, because gap-to-the-next-symbol handed it the whole
alignment hole and `--max-offset` has to be 1 MiB to accommodate release LTO's
inlined `kernel_main`. That is the mechanism behind the `font::FONT_DATA +
<hundreds of KiB>` frame. It now answers `?? (no symbol covers it; nearest
below is … at -0x3c28, size 0x10)`.

**The kind defect does not reproduce here, and the entry above guessed.** A
scan of all 119729 sized text symbols finds *zero* with a non-text symbol
interleaved inside their extent, so nearest-preceding could not have captured a
text address in this binary. Which means the sibling frame,
`kernel::KERNEL_BOOT_STACK+0xcc323 [b]`, was not caused by what this entry
said: `KERNEL_BOOT_STACK` records a **2 MiB** size, so `+0xcc323` lies genuinely
inside it and is the *correct* answer for a `.bss` address. For that frame to
have been a text address, the ELF consulted must have had a different layout
from the one that panicked — i.e. it was the **stale-ELF** failure, which
`pick_profile` fixed on the same day. Kind-aware search was still worth
building (it is free once the table is split, and a release build with jump
tables in `.text` can produce the interleaving), but it was not the bug.
Recording this because a fix credited to the wrong mechanism is how the real
mechanism survives.

**A second symbolizer existed, with the same bug, at the worse site.**
`boot-test.sh` carried its own awk implementation, `resolve_kernel_symbol`,
whose comment asserted that the last symbol with `addr <= RIP` "is the function
the RIP lies within". Its one caller is the **hang-capture** path: a boot that
wedged, one RIP from the QEMU monitor, nothing else to go on — the worst place
in the tree to print a plausible wrong name, because nothing contradicts it.
It has been deleted and the function now calls `symbolize.py`. (The one thing
worth keeping, the "not a higher-half address, likely ring 3" hint, stayed in
bash, since `symbolize.py` does not know this project's address-space split.)

This is the same shape as `BUG-SPAWNED-CHILDREN-INHERIT-NO-CAPABILITIES` fixed
hours earlier: **two implementations of one operation, an invariant that they
agree, and nothing that checks it.** Both were found by accident, from the
opposite end. Worth a habit: when a second implementation of an existing
operation appears, the question is which invariants of the first it silently
opted out of.

**Regression cover.** `python scripts/symbolize.py --self-test` checks that a
sampled function covers its own first and last byte, that an address in the
padding *after* a symbol is not attributed to it, that size-0 text symbols (the
naked-asm ISR stubs — exactly where an early-boot fault lands) still resolve,
and that `text_only` never answers with a data symbol. It was mutation-tested:
restoring gap-to-the-next-symbol in the lookup turns check 2 red and leaves the
others green, so the check fails for the reason it claims to.
