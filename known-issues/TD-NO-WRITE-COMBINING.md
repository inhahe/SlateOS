## TD-NO-WRITE-COMBINING — the kernel cannot ask for write-combining memory, so every framebuffer is uncached

**Lane:** A. **Status:** open. **Found:** 2026-08-17, writing
`kernel/src/drm/ati/aperture.rs`.

**What it is.** x86-64 picks a page's caching behaviour from three PTE bits
(`PAT`, `PCD`, `PWT`) that index an eight-entry table in the `IA32_PAT` MSR
(`0x277`). This kernel never writes that MSR, so the table keeps its power-on
contents — `WB, WT, UC-, UC, WB, WT, UC-, UC` — in which **no entry is
write-combining**. `PageFlags` correspondingly offers only `WRITE_THROUGH` and
`NO_CACHE`, and nothing in `kernel/src` mentions PAT or MTRRs.

**Why it matters.** Write-combining is the memory type a framebuffer wants: the
CPU gathers consecutive stores in a fill buffer and pushes them out as full
cache-line bursts, which for the sequential writes a framebuffer gets is
typically several times faster than uncached. Without it the choices are:

| Type | Correct for a framebuffer? | Speed |
|---|---|---|
| `NO_CACHE` (UC) | yes | one bus transaction per store |
| `WRITE_THROUGH` (WT) | yes | no faster for writes |
| writeback (WB) | **no** — pixels sit in a cache line the CRTC cannot see | fast |

So the only correct options are the slow ones, and `aperture.rs` takes `UC`.

**Where it bites.** `VramAperture::map` (`kernel/src/drm/ati/aperture.rs`) maps
BAR0 `NO_CACHE`. The boot-time display exercise paints at 640x480 rather than
anything larger specifically because of this — 1.2 MB of uncached stores is a
few tens of milliseconds and a 1080p buffer would be seconds. Any future
software-rendered path onto a real card hits the same wall. `kernel/src/mm/dma.rs:15`
already names write-combining as something the kernel ought to offer.

**The proper fix.** Program `IA32_PAT` early in boot (before any mapping is
established, or with a full TLB and cache flush after) to place `WC` in one
entry — Linux's layout puts it at index 1, selected by `PAT=0, PCD=0, PWT=1`,
which conveniently leaves entries 0, 2 and 3 at their power-on meanings so all
existing mappings keep their behaviour. Then add `PageFlags::WRITE_COMBINING`
setting `PWT` alone, and switch `aperture.rs` to it. The PTE-level `PAT` bit is
bit 7, which collides with `HUGE_PAGE` at the PD/PDPT levels — so either
confine `WC` to 4 KiB leaves or handle the collision explicitly.

**How to know it worked.** Time a full-screen fill through the aperture before
and after, at a fixed resolution; the whole point is the ratio. If it is not a
clear multiple, the PAT entry is not actually being selected — which is easy to
get wrong silently, since a wrong index just yields a different valid memory
type rather than a fault.
