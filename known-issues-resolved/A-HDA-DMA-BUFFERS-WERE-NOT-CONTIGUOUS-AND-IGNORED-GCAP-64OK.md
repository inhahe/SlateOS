## A-HDA-DMA-BUFFERS-WERE-NOT-CONTIGUOUS-AND-IGNORED-GCAP-64OK (lane A)

**Status:** FIXED 2026-08-18 (`2fe6f3c69`) — boot test `bjz6yilqr` PASSED on
`lane-a`. Move to `known-issues-resolved.md` once it has booted on `main`.

**In short:** the HD-audio driver asked the memory allocator for four separate
16 KiB pages and then told the sound hardware it had one continuous 64 KiB
buffer. Sometimes those pages happen to sit next to each other and nothing goes
wrong; when they do not, the audio hardware writes sound data over 48 KiB of
memory belonging to something else, with nothing to report it. On boot 91 the
driver's own zeroing of that "64 KiB" ran off the end of a usable memory region
and took the kernel down with a page fault — which is the *lucky* outcome,
because it is the only one that is visible.

### What was wrong

`kernel/src/hda.rs` allocated CORB, RIRB, BDL and the PCM output ring with four
independent `mm::frame::alloc_frame()` calls (one 16 KiB frame each), then:

* `write_bytes(pcm_virt, 0, 65536)` — zeroing 64 KiB from the *first* frame's
  HHDM address, i.e. across three frames it did not own;
* programmed a single BDL entry with `length: dev.pcm_size` (64 KiB),
  `SD_CBL = dev.pcm_size`, `SD_LVI = 0` — telling the DMA engine, in hardware,
  that one contiguous 64 KiB buffer lives at `pcm_phys`.

The buddy allocator makes no contiguity promise across separate `alloc_frame`
calls, so the claim was true only by luck.

Second, independent defect: GCAP bit 0 (`64OK`) was never read, yet
`setup_corb`/`setup_rirb` write the `*UBASE` (upper 32 bits) registers
unconditionally. On a controller with 64OK clear those registers are reserved
and the address is truncated to 32 bits — and this machine has usable RAM up to
`0x1_4000_0000`, so a buffer above 4 GiB would have been silently aimed at the
wrong physical address.

### How it showed up

Boot 91, `--bench`, release:

```
EXCEPTION: Page Fault (#PF) at 0xffffffff811a4089, address=0xffff80007fef4000, error=0x2
FATAL: Unrecoverable kernel page fault. Halting.
```

`0xffff80007fef4000` is HHDM + `0x7fef4000`, exactly the end of the usable
region `[0x7feb6000, 0x7fef4000)`. The run's PCM frame landed near the top of
that region and the 64 KiB zeroing walked past it. The faulting RIP resolved to
`kernel_main+0x2330c` — release LTO inlines the whole boot path — which is what
`scripts/symbolize.py` was written for; before it existed this was triaged only
from the surrounding serial lines.

Note the trigger: **where the allocator happens to land**, which is a function
of every allocation made before HDA init. So an unrelated change anywhere above
it in the boot order flips this between "works" and "kernel panic", and it had
been latent for a long time.

### The fix

All four buffers now go through `mm::dma::alloc(size, constraint)`, which
returns one contiguous, naturally-aligned, **zeroed** buddy block and charges it
to `MemType::DmaBuf`. The sizes are named constants (`CORB_BYTES`, `RIRB_BYTES`,
`BDL_BYTES`, `PCM_BYTES`) because each is written twice — once to size the
allocation and once to program a controller register — and the two silently
disagreeing is precisely how a DMA engine ends up ringing past the end of its
own buffer. The constraint is now `DmaConstraint::None` when GCAP `64OK` is set
and `DmaConstraint::Below4G` when it is not. The `DmaBuffer`s are held in the
device struct for the lifetime of the kernel with no free path, documented:
dropping one while `CORBCTL.RUN` is set would hand the allocator memory the
device is still writing to.

### Checked, and found correct

The sibling audio drivers were audited for the same shape:
`kernel/src/ac97.rs` and `kernel/src/virtio/sound.rs` each memset exactly
`FRAME_SIZE` from a single frame, so neither has the bug.
