### [A] RESOLVED — KASAN's own shadow-page bootstrap wrote through an *instrumented* `core::ptr::write_bytes`, so backing a shadow page reported a use-after-free against KASAN's bookkeeping — 2026-08-19

**Id:** `B-KASAN-SHADOW-BOOTSTRAP-SELF-REPORT`

**In short:** When KASAN needs a new page of its own bookkeeping memory, it asks
the frame allocator for a frame and zeroes it. That zeroing was done with a
`core` library call — and in the instrumented build, `core` calls compiled into
the kernel *are* checked by KASAN. So KASAN checked its own bookkeeping write,
against a frame that had just come back from the allocator and could still be
marked "freed" from whatever last used it, and reported a use-after-free that
was entirely its own doing. The whole point of `mm::rawmem` is that this class
of call must not appear in the memory-debugging modules; two had survived inside
`mm::kasan` itself.

#### Where

`kernel/src/mm/kasan.rs`:

| Site | Call | Consequence |
|---|---|---|
| `ensure_shadow_mapped` | `core::ptr::write_bytes(frame_virt, 0, FRAME_SIZE)` on the **HHDM alias of a freshly-allocated frame** | genuine false report — the alias is inside the covered window and the frame may carry stale `0xFA` |
| `set_shadow` | `core::ptr::write_volatile(sv, val)` on a **shadow byte** | no report today, because the shadow window lies outside the covered range — but that is a coincidence of the address-space layout, not an invariant anyone maintains |

Both are now `crate::mm::rawmem::fill_u8` / `write_u8`.

#### Why the module-level opt-out did not cover them

This is exactly the hazard `mm::rawmem`'s module docs and `design-decisions.md`
§118/§119 were written for, restated:

> A module-level `sanitize` attribute cannot exempt a generic `core` function.
> `sanitize` is a per-function LLVM attribute, and a generic `core` function
> monomorphises into *this* crate carrying the default (instrumented)
> attribute — the exempt module's attribute never applies to it.

`mm::kasan` carries `#![cfg_attr(kasan_instrumented, sanitize(address = "off"))]`
at line 80 and reads, from its own module docs down, as though that settles the
question. It does not, and the two survivors were found by grepping for the
pattern rather than by reading the file — the same lesson as
`B-KASAN-FALSE-CLEAN-STREAK`: a rule stated in prose is not a rule that is
enforced.

**The check that would have caught it:** `grep -n "core::ptr::" ` over every
module carrying the `sanitize(address = "off")` opt-out. That is one command and
it should have been run when `mm::rawmem` was introduced, since introducing it
was the admission that these call sites are bugs. It is worth wiring into
`scripts/` as a lint rather than leaving as a thing to remember.

#### Relationship to the frame-reuse bug

`B-KASAN-POISON-SURVIVES-FRAME-REUSE` is the reason the frame `ensure_shadow_mapped`
allocates could be poisoned at all. Fixing that removes most of the fuel;
fixing this removes the ignition. Both are needed: the frame hook cannot cover
`ensure_shadow_mapped`'s own frame, because that allocation happens *inside* the
hook's re-entrancy guard and is deliberately skipped.
