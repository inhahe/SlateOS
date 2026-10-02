### TD-FRAME-OWNER-1GIB. `frame_owner` ownership array only tracks the first 1 GiB of RAM (fixed `[u8; 65536]`) — 2026-07-22 — ✅ RESOLVED 2026-08-14 (array made dynamic *and* wired into the allocator)

**Where:** `kernel/src/mm/frame_owner.rs` — `const MAX_FRAMES: usize = 65536;`
and the `OwnerArray([u8; MAX_FRAMES])` static; `set`/`get`/`clear` no-op when
`frame_idx >= MAX_FRAMES`.

**What:** Frame index = `phys_addr / FRAME_SIZE` (16 KiB), so 65536 frames covers
only the first **1 GiB** of physical memory. On any machine with more RAM, a
frame allocated above 1 GiB gets owner `Unknown` — its owner tag is silently
dropped. This is the *same* fixed-window bug that affected the per-frame cgroup
array (now fixed — see BUG-CGROUP-1GIB below), but for the frame *owner*
tracking.

**Impact:** ownership tracking is **diagnostic only** (leak reporting, the
`[mm]` owner-census stats, and the owner self-test), so a wrong `Unknown` above
1 GiB degrades diagnostics but does **not** corrupt allocation or accounting.
That is why this is low severity and was left open while the cgroup array (which
*is* correctness-affecting) was fixed immediately.

**Even lower priority than it looks:** `frame_owner::set`/`clear` currently have
**no callers** in the allocator (verified 2026-07-22 — the `Owner` enum is only
passed as a label to the separate `alloc_trace` ring buffer). So the `OWNERS`
array is never populated in production; it is exercised only by the module's own
self-test. The 1-GiB ceiling therefore has no real effect today. The proper fix
below should be done *together with* wiring `set`/`clear` into the alloc/free
paths if per-frame owner census is ever actually wanted; refactoring the array
to be dynamic in isolation (while it stays unwired) is busywork.

**Proper fix:** make `OwnerArray` dynamic exactly like the cgroup array now is —
carve `total_frames` bytes from the frame-allocator metadata region in
`frame::init` (or a dedicated init hook), publish a base-pointer + length pair,
and bounds-check `set`/`get`/`clear` against the dynamic length. The metadata
region already reserves per-frame bytes; adding one more `total_frames`-byte
sub-array is the same pattern used for `page_info`/`refcount`/cgroup.

---

**RESOLVED 2026-08-14.** Both halves were done together, exactly as the note
above insisted — resizing the array alone would have been busywork while
`set`/`clear` still had no callers.

**1. Dynamic array.** `const MAX_FRAMES = 65536` and the
`OwnerArray([u8; MAX_FRAMES])` static are gone. `frame::plan_metadata` now
reserves a fourth per-frame sub-array (`owner_offset = cgroup_offset +
total_frames`), `frame::init` zeroes it (`0` == `Owner::Free`) and publishes it
via `frame_owner::init_storage(ptr, total_frames)`; `OWNERS_PTR`/`OWNERS_LEN`
back a `slot()` helper that every accessor goes through. Same pattern as the
cgroup array (BUG-CGROUP-1GIB). Boot confirms the carve:
`[mm] Metadata: ... [page_info: 327680B, refcount: 655360B, cgroup: 327680B, owner: 327680B]`.

**2. Actually wired up.** `tag_alloc_owner`/`untag_free_owner` are called from
all six allocator choke points: both per-CPU fast paths in `alloc_frame`, the
zero-pool pop in `alloc_frame_zeroed`, `alloc_order`, `alloc_order_constrained`,
`free_frame` and `free_order`. The zero-pool refiller tags parked frames
`Owner::ZeroPool`; the consumer re-tags on pop.

**3. Ambient owner context.** The allocator cannot know its caller, so
attribution comes from `OwnerScope` — a cache-line-padded per-CPU RAII guard
that saves the previous tag and restores it on drop, so it nests correctly even
when an IRQ handler allocates inside another subsystem's scope. Tagged so far:
page tables (`page_table.rs` PT-page pool refill), kernel stacks (`kstack.rs`),
slab + large heap (`heap.rs`), CoW (`cow.rs`), and user anon pages (`vma.rs`
demand paging, `idt.rs` stack growth). Untagged allocations record
`Owner::Unknown`, which is honest rather than wrong.

Known accuracy limit, documented on `OwnerScope`: the tag is per-CPU, so a task
preempted and migrated mid-scope restores onto the new CPU and can mis-attribute
a handful of frames. Accepted deliberately — this is diagnostic-only, and a lock
or a per-task field reachable from boot/IRQ contexts would cost more on the
allocation hot path than the precision is worth.

**Verified.** Boot PASSED 273s; the rewritten self-test reports:

```
[frame_owner]   Covers all 327680 frames (5120 MiB): OK
[frame_owner]   High frame 327679 (> old 65536-frame window): OK
[frame_owner]   Alloc/free tagging round-trip: OK
[frame_owner]   OwnerScope nesting: OK
[frame_owner]   summary/find_by_owner: OK
[frame_owner]   Stats: sets=155837, clears=299809
```

The second line is the direct regression test for this bug. The nonzero
`sets`/`clears` are the proof that the allocator now reaches this module at all.

*On `clears` > `sets`:* these count **calls, not transitions**. `clear()` runs on
every free, including frames that were never tagged — anything allocated before
`init_storage` published the array, plus rollback paths that free via
`free_order_inner` without a matching tagged alloc. Not a leak; the per-frame
state is still a correct free/allocated mirror, as test 4's round-trip shows.

**The self-test had to be rewritten, not just extended.** The old one wrote raw
indices (100, 200, 300…) straight into the array. That was harmless while
nothing populated it, but the moment the allocator went live those writes would
corrupt *real* frames' records. It now allocates and frees actual frames for
every check, and saves/restores around the one raw-index probe.

**Side effect: found and fixed a latent bug.** Making `current_owner()` run on
every allocation pulled `smp::fast_cpu_index()` into early boot and exposed
B-SMP-FAST-CPU-INDEX-PANICS-BEFORE-APIC-INIT (tier-3 APIC fallback reads a null
APIC base before `apic::init` — panic in debug, wild read in release). See that
entry.
