## §218 — GEM backing became an enum, because "a list of physical frames" is a lie about video memory that the buddy allocator would believe

**Date:** 2026-08-17
**Decided by:** Claude (autonomous)
**Lane:** C

**In short:** A "GEM object" is the kernel's word for a chunk of memory a
graphics driver hands out to hold an image. Until now every one of them was a
list of ordinary RAM pages, and freeing one meant giving those pages back to
the system's memory allocator. The new ATI driver hands out memory that lives
*on the graphics card* instead — which is not RAM, is not the system
allocator's, and would poison it if returned there. The decision was to make
the difference visible in the type, so the compiler asks every piece of code
which of the two kinds it is prepared to deal with, rather than letting card
memory quietly flow down a path built for RAM.

### The concrete hazard

`GemObject` carried `phys_frames: Vec<PhysFrame>`, and `free_backing()` walked
it calling `frame::free_frame`. The obvious way to add VRAM support is to put
the card's addresses in that same vector — they *are* physical addresses, and
`PhysFrame::from_addr` accepts them.

The result would compile, run, and pass a functional test. It would also, on
the first `gem_destroy`, insert a run of PCI BAR addresses into the buddy
allocator's free lists. The next unrelated `alloc_frame()` anywhere in the
kernel returns one of them, and a kernel structure gets built in the graphics
card's framebuffer. The symptom is arbitrary corruption, appearing at an
arbitrary later time, in a subsystem with no connection to graphics.

That is worse than a leak by a wide margin: a leak is bounded, visible in a
counter, and diagnosable. This is unbounded, invisible, and the evidence points
everywhere except at the driver responsible.

### Options considered

| | *What changes:* |
|---|---|
| **A. Chosen — `GemBacking` enum** | `phys_frames` becomes `backing: GemBacking`, with `SystemRam(Vec<PhysFrame>)` and `Vram { offset, len }`. Every consumer of the frame list must say what it does about VRAM; `free_backing` returns `NotSupported` rather than freeing. |
| B. Leave `phys_frames`, add a `is_vram: bool` beside it | Nothing structural. The vector still exists and is still the obvious thing to iterate; the flag is a comment the compiler does not enforce. |
| C. Leave `phys_frames` empty for VRAM objects, add `vram_offset: Option<u32>` | `free_backing` becomes a no-op on VRAM (correct), but every *reader* silently sees a zero-length buffer instead of an error. A compositor asking for the frames of a VRAM buffer gets "no pixels" rather than "wrong question". |

### Why A over C

C is genuinely safe against the corruption — an empty vector cannot be freed
wrongly — and it is a much smaller change. It was rejected because it converts
the hazard from corruption into *silence*, which is the failure mode this
codebase keeps deciding against elsewhere (see the aperture's refusal to assume
its `NO_CACHE` request was granted, §217's whole premise, and the mode list's
refusal to advertise a mode that would then be refused).

Concretely: `DrmDevice::gem_frame_addrs` under C returns `Ok(vec![])` for a
VRAM object. A caller loops over it, writes nothing, and reports success. The
screen stays black and there is no error anywhere to find. Under A the same
call returns `NotSupported`, which names the mistake at the point it is made.

The cost of A is real and was paid: eleven call sites across `driver.rs`,
`mod.rs` and `gem.rs` had to be examined and told what to do. That is the
benefit, not the price — each of those was a place where the question "what if
this is video memory?" had never been asked, and four of them (the Limine and
virtio page-flip and flush-region loops) turned out to need an explicit refusal
they did not previously have.

### Why B was never really in the running

A `bool` beside the vector documents the invariant without enforcing it. The
next person to add a backend reads `phys_frames`, iterates it, and the flag
does nothing. It is exactly the "quick fix that becomes permanent" the project
standards rule out.

### What this does not decide

VRAM objects currently cannot be mapped into a user process:
`gem_phys_addrs` returns `NotSupported` for them. Their bytes *do* have
physical addresses, but the `mmap` shim would map them with ordinary
write-back caching, and pixels written into a cache line are pixels the scanout
engine never sees. Fixing that needs the write-combining page attribute tracked
as `TD-NO-WRITE-COMBINING` in `known-issues.md`; until it exists, refusing is
the honest answer.
