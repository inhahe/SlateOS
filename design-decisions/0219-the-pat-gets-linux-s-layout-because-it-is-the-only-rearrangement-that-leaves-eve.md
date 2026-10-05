## §219 — The PAT gets Linux's layout, because it is the only rearrangement that leaves every mapping the kernel has already made meaning exactly what it meant before

**Date:** 2026-08-17
**Decided by:** Claude (autonomous)
**Lane:** C

**In short:** The graphics card's memory was being written to one pixel at a
time over a slow bus, because the CPU had no way to be told "batch these writes
up and send them in bulk." x86 does have a mode for that — *write-combining* —
but it lives in an eight-slot table that, on power-up, does not contain a
write-combining entry at all. So the mode had to be created before it could be
used. The catch is that the same eight slots are already being selected by every
memory mapping in the kernel: rewrite a slot and you silently change what
existing, working mappings mean. The decision was **which** slot to sacrifice.
The answer is the one Linux picked, for a reason that can be checked rather than
taken on faith.

**Background, in plain terms.** Every page of memory has a *caching mode* — how
freely the CPU is allowed to hold on to writes instead of sending them out
immediately. Three bits in the page's descriptor (`PAT`, `PCD`, `PWT`) do *not*
name the mode; together they form a 3-bit number, 0–7, which indexes a table of
eight entries held in a CPU register (`IA32_PAT`, MSR `0x277`). The page picks a
slot; the register decides what the slot means. Nothing about that table is
fixed — which is what makes it both useful and dangerous.

Power-on contents are `WB, WT, UC-, UC, WB, WT, UC-, UC`
(WB = writeback, the fast default; WT = write-through; UC = uncacheable;
UC- = uncacheable-but-MTRR-overridable). Note what is missing: **WC
(write-combining) is not in the table.** There is no bit pattern a page can
carry that means write-combining until the register is rewritten.

**The decision.** Program the table to Linux's layout,
`WB, WC, UC-, UC, WB, WP, UC-, WT` (`arch/x86/mm/pat/memtype.c`, `pat_init`).

**Why this layout and not one of our own choosing — the checkable argument.**
The property that matters is not elegance, it is that *no mapping already made
changes meaning*. Under this layout:

| Slot | Bits selecting it | Power-on | New | Mappings affected |
|---|---|---|---|---|
| 0 | none | WB | WB | unchanged — all ordinary memory |
| 1 | `PWT` | WT | **WC** | **the one change** |
| 2 | `PCD` | UC- | UC- | unchanged — all 15 `NO_CACHE` MMIO sites |
| 3 | `PCD`+`PWT` | UC | UC | unchanged |
| 4–7 | require `PAT` (bit 7) | — | — | none — nothing sets bit 7 |

Slots 0, 2 and 3 keep their power-on meanings, and those are the only slots any
live mapping selects. Slots 4–7 require bit 7, which no existing flag set uses.
So exactly one slot changes, and it can be enumerated: slot 1, `PWT` alone,
write-through → write-combining.

**The one hazard, and why redefining a constant defuses it.** Write-through's
whole point is *stronger* ordering; write-combining is *weaker*. A caller that
asked for write-through and silently got write-combining would be a real bug,
and nothing would fault — the wrongness would show up as a device seeing writes
in the wrong order, arbitrarily later. The kernel has exactly one such caller,
`mm::dma::alloc_for_user`, and it asks by name: `PageFlags::WRITE_THROUGH`. So
the same commit that rewrites the MSR also moves that constant from `1 << 3`
(slot 1, now WC) to `(1 << 7) | (1 << 4) | (1 << 3)` (slot 7, still WT), and the
caller follows automatically. `PageFlags::WRITE_COMBINING` takes over the vacated
`1 << 3`.

That leaves a trap for whoever writes the *next* such mapping: hard-code `1 << 3`
believing it to mean write-through and you get write-combining with no
diagnostic. Two guards, because a comment alone would not survive:
`WRITE_THROUGH`'s doc comment states the relocation explicitly, and
`mm::pat::self_test` decodes all four named constants back through the table in
force and fails the boot if any of them stops meaning what its name says. The
self-test is the part that matters — it turns "someone split the MSR write from
the constant change" from a silent miscompile into a boot failure.

**Alternatives considered.**

* **Put WC in a slot that needs bit 7 (4–7) and leave slots 0–3 completely
  untouched.** Superficially the safest option, and rejected for a specific
  reason: bit 7 is not free. At the page-table (leaf) level bit 7 *is* `PAT`, but
  at the directory level the same bit is the page-size bit, and `PAT` moves to
  bit 12. A flag set carrying bit 7 that reaches a huge-page mapper does not mean
  "write-combining", it means "already a huge page", and the result is an
  *uncacheable* huge page created silently. SlateOS's 16 KiB frames are four
  4 KiB PTEs, so every leaf we map is 4 KiB and bit 7 is safe *there* — but
  building the common, hot, frequently-copied WC flag on a bit that is only safe
  by virtue of a frame-size coincidence is the wrong foundation. Slot 1 needs no
  such reasoning. (Bit 7 is still used, for `WRITE_THROUGH` — but that has one
  caller in the whole kernel, not the open-ended set WC will accumulate, and
  `map_huge_2m` now rejects any flag set carrying bit 7 outright so the collision
  is a returned error rather than a wrong mapping.)
* **Invent our own layout.** No benefit, and it discards the one genuinely
  valuable property of matching Linux: every firmware, hypervisor and errata
  workaround in existence has been exercised against *this* table. QEMU included.
* **Use MTRRs instead of the PAT.** MTRRs are a fixed, small set of physical
  ranges, allocated at boot and awkward to change; the PAT is per-page and needs
  no range bookkeeping. For a framebuffer aperture whose size and location come
  from a PCI BAR at probe time, per-page is the right granularity.

**Two consequences worth stating, because both are silent if forgotten.**

1. **The PAT is per-logical-processor.** A page descriptor stores only the
   3-bit index, so a core still on the power-on layout reads a WC page as
   *write-through*. That disagreement is invisible until a thread migrates, so
   `mm::pat::init_ap` programs each AP, gated on the BSP having succeeded — an AP
   never creates the disagreement on its own.
2. **The decoder must report the table actually in force, not the one we intend
   — and "in force" is not the same as "architectural default".** The first
   boot with this code logged the pre-existing table as `0x0000010500070406`,
   whose slots 4–7 are `WP, WC, UC, UC`. The architectural power-on table says
   those slots are `WB, WT, UC-, UC`. So the firmware/bootloader had already
   rewritten half the table before the kernel ran, and a decoder falling back to
   the architectural default would have reported four slots as memory types they
   demonstrably were not — with no way for a reader to tell. `init` therefore
   saves the value it read *before* overwriting it, and `memory_type_of` decodes
   against that when `init` has not run. A diagnostic that is confidently wrong
   is worse than one that says "WT", and worse still when the wrongness comes
   from trusting a specification over an observation.

**What this buys.** The ATI framebuffer aperture
(`kernel/src/drm/ati/aperture.rs`) maps BAR0 write-combining instead of
uncached. Uncached stores to a PCI BAR go out individually; write-combining
gathers them in a fill buffer and burst-writes a full cache line. The cost is
that the unconditional `sfence` in `Aperture::flush` becomes load-bearing rather
than belt-and-braces: WC stores sit in that fill buffer with no ordering
guarantee, so the `CRTC_OFFSET` register write that publishes a frame must not
be allowed to overtake the pixels it publishes.
