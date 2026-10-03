### TD-NO-WRITE-COMBINING -- UPDATE 2026-08-17 -- implemented as `kernel/src/mm/pat.rs`; the speed *ratio* this entry asks for is not yet measured

`mm::pat` now programs `IA32_PAT` to Linux's layout on the BSP (`init`) and on
every AP (`init_ap`), `PageFlags::WRITE_COMBINING` selects slot 1, and
`drm/ati/aperture.rs` maps BAR0 with it. Rationale, the alternatives, and the
argument for why no *existing* mapping changes meaning: `design-decisions.md`
§219. Validated by boot #11 (PASS, 493 s).

Two corrections to the plan sketched above, both discovered by doing it:

1. **The bit-7 collision was handled by not relying on bit 7 for WC.** The
   sketch offered "confine `WC` to 4 KiB leaves or handle the collision
   explicitly". Slot 1 needs no `PAT` bit at all, so `WRITE_COMBINING` is
   `PWT` alone and never collides. Bit 7 is now used only by
   `PageFlags::WRITE_THROUGH`, which moved to slot 7 to vacate slot 1 -- one
   caller in the whole kernel (`mm::dma::alloc_for_user`), which asks by name.
   `mm::hugepage::map_huge_2m` additionally rejects any flag set carrying bit 7
   outright, so a `WRITE_THROUGH` request reaching a huge-page mapper returns
   `InvalidArgument` instead of silently producing an *uncacheable* huge page.
2. **"Power-on meanings" is not what the kernel actually inherits.** The first
   boot logged the pre-existing table as `0x0000010500070406` -- slots 4-7 are
   `WP, WC, UC, UC`, where the architectural power-on table says
   `WB, WT, UC-, UC`. The firmware/bootloader had already rewritten half of it.
   The claim that matters survives (slots 0-3 *were* at their power-on values,
   and those are the only slots any live mapping selects), but any future
   reasoning of the form "the other entries are at their power-on values" is
   unsound on this boot path. `mm::pat::init` now saves the value it read before
   overwriting it, and `memory_type_of` decodes against *that* when `init` has
   not run, rather than against a specification the machine contradicts.

**Still open, and the reason this entry is not closed.** The acceptance test
this entry itself specifies -- "time a full-screen fill through the aperture
before and after; the whole point is the ratio" -- has not been run. What is
verified so far is weaker, and the difference is exactly the failure mode the
entry warned about:

* `mm::pat::self_test` confirms the MSR reads back as programmed, and that the
  four named `PageFlags` constants still decode to `WB`/`WC`/`WT`/`UC` through
  the table in force. That proves the *table* is right and the *flags* select
  the slots we think they do.
* It does **not** prove the CPU is combining. A wrong index yields a different
  valid memory type rather than a fault, and `UC` and `WC` are
  indistinguishable from any decode-side check -- both report "writes do not
  linger", which is why `Aperture::flush` skips `clflush` for both.

The measurement to add: time a full-screen fill through the WC aperture and
through an uncached mapping of the *same* BAR in the same boot, and report the
ratio. Doing both in one boot avoids comparing against a number from a different
build, and a ratio near 1.0 is then unambiguous evidence that slot 1 is not
being selected -- the exact silent failure this entry predicted.
