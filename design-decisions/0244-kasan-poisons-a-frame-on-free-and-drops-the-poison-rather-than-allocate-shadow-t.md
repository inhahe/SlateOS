## §244 — KASAN poisons a frame on free, and drops the poison rather than allocate shadow to record it

**Date:** 2026-08-19
**Decided by:** Claude (autonomous)
**Lane:** A

**In short.** KASAN is the debug build's memory-error detector: it keeps a
"shadow" byte for every 8 bytes of memory saying whether that memory is
currently allowed to be touched, and reports any access that says otherwise.
Until now it only marked *heap objects* as freed, so a stale pointer into
memory that was owned a whole 16 KiB page at a time — page tables, per-process
address-space backing, kernel stacks — read as perfectly valid and no report
fired. This change marks such a page as freed when the allocator takes it back.
The tradeoff decided here is what to do in the case where the shadow bookkeeping
for that page has not itself been allocated yet: allocate it (which means the
page allocator calls itself, from inside its own free path), or give up on that
one page and record nothing. We give up.

**Decision.** `mm::kasan::on_frame_free` writes `KASAN_FREE` (`0xFA`) over the
shadow of every frame the allocator reclaims from its *last* owner, using a new
`IfUnmapped::SkipLossy`: ranges whose shadow frame is not already backed are
skipped, and the poison for them is discarded.

**Why not `IfUnmapped::Map` (back the shadow, never lose a poison).** Mapping a
shadow frame calls `frame::alloc_frame`. A poisoner that maps is therefore a
poisoner re-entered through the frame allocator *while that allocator is midway
through a free* — per-CPU cache and buddy lists in flux, and, on the fast path,
about to disable interrupts. §-numbered history is unambiguous about how that
goes: the first version of the *unpoison* hook mapped, needed a per-CPU
recursion guard plus an interrupts-off window to survive it, and that window
forced every call to reach `MAP_LOCK` in exactly the state whose give-up path
drops the operation. It shipped `map_lock_giveups=3` on its first boot. The free
path is a strictly worse place to recurse than the alloc path, so the same
design would be at least as bad here.

**Why losing a poison is acceptable when losing an unpoison was not.** The two
hooks fail in opposite directions, and that — not the value written — is the
whole argument:

| | dropped write costs | resulting behaviour |
|---|---|---|
| unpoison (alloc) | stale `0xFA` stays on memory that is now live | **fails closed**: every later legitimate access to that frame is reported, forever |
| poison (free) | `0xFA` is never written | **fails open**: one frame goes unwatched — exactly the behaviour that existed before this hook |

A dropped poison can never manufacture a report that should not have fired. It
can only fail to produce one, on a frame whose shadow was never backed — which
is to say, on a frame no checked object has ever lived in. The frames that
matter most are precisely the ones already backed, because backing is what the
heap's own poison-on-free does.

`SkipLossy` is a separate variant from `Skip` rather than a relaxation of it
because `Skip`'s correctness rests on an *exact equivalence* — an unbacked
shadow frame already reads `0x00` through the shared zero page, so writing
`KASAN_ADDRESSABLE` into it changes nothing. That argument does not survive a
non-zero value, and a `debug_assert!` enforces that only `SkipLossy` may carry
one. Collapsing the two would erase the reason the unpoison hook is allowed to
skip, which is the one place where skipping is free rather than lossy.

**Why the sole-owner predicate is hoisted out of `is_zero_on_free()`.** A frame
with refcount > 1 is still live for its other owners — copy-on-write mappings
share frames by design — so poisoning it would slander their entirely
legitimate accesses. That is the same predicate zero-on-free already applies,
for the same reason (both would ruin a still-live frame), so the two now share
`block_is_sole_owned`. They are deliberately *not* coupled beyond that:
zero-on-free is a configurable hardening policy and this is a debug-build
detection claim, and a detector that switches itself off because an unrelated
hardening knob is off is a detector that reads clean for the wrong reason.
Before this change the predicate lived *inside* the `is_zero_on_free()` gate,
so reusing it as-is would have done exactly that.

**Why this was deferred until now, and what unblocked it.** `known-issues.md`
set an explicit trigger: *"Land the unpoison side, get a clean whole-boot
instrumented run, and consider poisoning-on-free as a separate change with its
own boot evidence."* The concern was that the buddy allocator writes its own
freelist links and its zero-on-free memset through the HHDM alias of frames it
has just freed, which would report against itself. That turned out to be
already defused: `kernel/src/mm/frame.rs` carries a module-scope
`#![cfg_attr(kasan_instrumented, sanitize(address = "off"))]`, so none of the
allocator's own accesses are checked. The trigger's other half was met on its
own terms — a full instrumented boot reached `BOOT_OK` with exactly three
reports, all of them the deliberate self-tests, and `map_lock_giveups=0`.

**What would change this decision.** A measurement showing that a material
fraction of freed frames have unbacked shadow *and* that real bugs are being
missed because of it. The fix then is not to make the hook map — it is to back
the shadow eagerly at a point where allocating is safe, which decouples the
question from the free path entirely.
