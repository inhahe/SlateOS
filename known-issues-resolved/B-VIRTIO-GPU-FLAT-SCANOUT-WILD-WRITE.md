## B-VIRTIO-GPU-FLAT-SCANOUT-WILD-WRITE (lane A, 2026-08-21) — FIXED 2026-08-21

**In short:** the virtio-gpu display driver stores the screen's pixels in 250
separate 16 KiB chunks scattered around RAM, but the code that copied a picture
onto the screen assumed those chunks were one continuous 4 MiB block. So it
wrote the first 16 KiB into the screen and the remaining ~4 MiB straight over
whatever else the kernel happened to have put next in memory. It is fixed, and
it is very likely the cause of the intermittent heap corruption that has been
chased since July under the name **B-KNULLJUMP**.

### What was wrong

`virtio::gpu` builds the scanout out of frames taken one at a time from the
buddy allocator:

```rust
let frames_needed = fb_bytes.div_ceil(FRAME_SIZE);
for i in 0..frames_needed {
    let f = frame::alloc_frame()?;   // <- unrelated frames, in allocation order
    fb_frames.push(f);
}
```

There is no contiguity request anywhere in that loop, so `fb_frames` is a list
of 250 physically unrelated frames at 1280×800.

`framebuffer_addr()` returned `fb_frames.first()?.addr() + hhdm` — the base of
**one** of those frames — and both blit paths in `kernel/src/drm/driver.rs`
then did flat-buffer arithmetic on it:

```rust
let dst_base = crate::virtio::gpu::framebuffer_addr()?;   // first frame only
let dst_row  = dst_base + (row * dst_pitch) as u64;       // rows 0..800
copy_nonoverlapping(src_virt as *const u8, dst_row as *mut u8, to_copy);
```

Rows 0–3 land in the framebuffer. Rows 4–799 land in the ~4 MiB of physical
memory that follows the first frame, which is other GEM frames, kernel heap
slabs, page-cache pages — whatever the allocator placed there.

`VirtioGpuBackend::flush_region` had the identical defect, plus two of its own:
`copy_w_bytes` was never clamped to the destination's right edge, and a source
row crossing a GEM frame boundary silently lost its tail (`page_flip` handled
exactly *one* such crossing, which is enough only while a row is ≤ 16 KiB).

`set_pixel` in the same file has always done it correctly —
`frame_idx = offset / FRAME_SIZE`, then `fb_frames.get(frame_idx)` — so the
right idiom was sitting twenty lines below the accessor that made the wrong one
easy.

### How it was found, and why it stayed hidden

Nothing at boot ever called `VirtioGpuBackend::page_flip`. The kernel console
draws through `set_pixel`, which is correct; the compositor's path had not run
by the time the DRM self-test does. The `SETCRTC`/`page_flip` self-test added
on 2026-08-21 (`drm::self_test` item 11) performs a *real* mode-set and a real
full-surface flip on the primary display — making it the first full-surface
virtio-gpu blit in the boot — and the boot then panicked, deterministically,
about 300 serial lines later:

```
panicked at alloc/src/collections/btree/navigate.rs:231:55:
unsafe precondition(s) violated: hint::unreachable_unchecked must never be reached
  kmain -> kernel_main -> oom::self_test -> oom::handle_oom
        -> mm::pressure::notify -> mm::page_cache::shrink
        -> BTreeMap Iter::next -> next_unchecked -> init_front
```

`navigate.rs:231` is the `LazyLeafHandle::Root(_) => unreachable_unchecked()`
arm of `init_front`: reaching it means the page-cache `BTreeMap`'s own nodes no
longer agree with each other. That is a corrupted heap, and the ~4 MiB of pixel
data sprayed over it a moment earlier is where it came from.

Two properties made the diagnosis solid rather than plausible:

* **It was deterministic across two different binaries.** Two builds that
  differed by an unrelated fix panicked at the same serial line (39337/39338)
  and the identical relative stack offset. Layout luck does not do that.
* **Gating item 11 off — and changing nothing else — made the boot green.**
  `[oom] Self-test PASSED`, `BOOT_OK`. That is what turned "my change is
  implicated" into "this code path is the trigger".

### The fix

The accessor was the trap, so the accessor is what went away. Patching the two
callers would have left the next one to make the same mistake — and two of the
three existing callers already had.

* `framebuffer_addr()` → **`first_frame_addr()`**, documented as "only the
  first `FRAME_SIZE` bytes belong to the framebuffer", kept solely for a
  bounded read inside frame 0 and for printing an address to a human.
* New **`with_scanout(|sc| …)`** lends a `ScanoutMem` view under the device
  lock. Its `write_at(dst_offset, src, len)` walks `fb_frames`, splits the copy
  at every frame boundary, and **clamps to the end of the buffer** — bytes past
  the end are discarded, so a caller with wrong arithmetic now draws a wrong
  picture instead of corrupting the kernel. It returns the byte count actually
  written, which is what the regression test asserts on.
* New `blit_run()` in `drm/driver.rs` walks the *source* GEM frame list in the
  same general way, so neither side is assumed flat and a row may cross any
  number of boundaries on either.
* `flush_region` additionally clamps the run to the right edge of both
  surfaces.
* `kshell`'s `gpu status` now prints the frame *count* alongside the first
  frame's address, because printing one bare base address is what invited the
  arithmetic in the first place.

### Regression test

`virtio::gpu::self_test` now writes a marker through `write_at` at the offset
of the **last** pixel and reads it back with `read_pixel`'s frame walk. Under
the old flat-buffer arithmetic those four bytes would land ~4 MiB past frame 0,
in someone else's memory, and the read-back would find zeros. It also asserts
that a `write_at` starting at `sc.len()` writes **0** bytes.

### Relationship to B-KNULLJUMP

Not proven identical, and this entry does not close B-KNULLJUMP. But
B-KNULLJUMP is an intermittent, hard-to-reproduce corruption of kernel heap
structures, and this is a multi-megabyte wild write in the display path that
fires whenever a full-surface virtio-gpu flip happens — which is exactly the
compositor's steady state. Anyone re-opening B-KNULLJUMP should first check
whether it still reproduces on a tree containing this fix.

### Direct confirmation (2026-08-22): caught in the act, one frame past the end

A KASAN-instrumented boot of the **pre-fix** kernel was left running as
independent evidence. It did not need to produce a KASAN report — it produced
something better, a hardware page fault at the exact instruction, with
arithmetic that admits only one explanation:

```
[drm]   Cursor operations: OK
EXCEPTION: Page Fault (#PF) at 0xffffffff824b96a3, address=0xffff80007feb0000, error=0x2
  Cause: not-present, write, kernel
  bytes @RIP (16): [f3, 48, a5, 83, e2, 07, 48, 89, d1, f3, a4, c3, ...]
```

Four facts, each independently checkable from the log:

1. **The faulting instruction is `memcpy`.** `f3 48 a5` is `rep movsq`, and the
   bytes that follow (`and edx,7` / `mov rcx,rdx` / `rep movsb` / `ret`) are the
   tail of a bulk copy. Nothing else in the kernel has that byte sequence.
2. **The destination was a single 16 KiB frame, and the fault is exactly one
   frame past its base.** `0xffff80007feac000` appears four times in the stack
   scan; the fault address is `0xffff80007feb0000`. The difference is `0x4000`
   — `FRAME_SIZE`, to the byte. The copy walked to the end of one frame and
   took one step more.
3. **That step left mapped RAM.** The boot memory map in the same log reads
   `[0x007fe46000 - 0x007feb0000] usable` — the usable region ends at
   `0x7feb0000`, the faulting physical address, with a reserved hole before the
   next region at `0x7feb6000`. The frame at `0x7feac000` is the *last* frame
   of its region, so the flat arithmetic's first step past it had nowhere to
   land.
4. **It happened in the DRM path.** The line immediately before is
   `[drm]   Cursor operations: OK`.

This is the same bug as the `BTreeMap` panic that started the hunt, seen under
a different heap layout. KASAN's shadow shifts every allocation, so on this
boot the frame that `framebuffer_addr()` returned happened to be the last one
in its region and the overrun hit an unmapped hole immediately; on the
uninstrumented boot it landed in live page-cache structures and corrupted them
silently, surfacing thousands of lines later inside `BTreeMap`. Same wild
write, two symptoms — which is exactly why it was intermittent.

Worth keeping as a diagnostic lesson: **a page fault whose address is a round
multiple of `FRAME_SIZE` above a value on the stack is an overrun of a single
frame, and the memory map says whether that frame was the last one in its
region.** That triangulation took minutes; the symptom-chasing before it took
days.
