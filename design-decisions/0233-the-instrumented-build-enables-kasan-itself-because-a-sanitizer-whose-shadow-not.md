## §233 — The instrumented build enables KASAN itself, because a sanitizer whose shadow nothing populates is not a weaker check but a guaranteed-silent one

**Date:** 2026-08-19
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** KASAN has two halves that have to agree. One half is the compiler:
built with `scripts/kasan-build.sh`, it inserts a check in front of every read
and write the kernel performs. The other half is our code: it maintains the
"shadow", a side table saying which bytes are currently usable, updated when
memory is allocated and freed. Those updates were switched off by default. So
the instrumented kernel dutifully checked every access against a table that was
blank — and a blank table means "everything is fine", always. The build took
about 3.5x as long to boot and could not, even in principle, report anything.
The decision is that the instrumented build now switches the shadow updates on
by itself, for the whole boot, instead of waiting for a command-line flag that
nobody passing `--boot` had any reason to think was required.

### What was actually there

`mm::kasan::on_alloc` and `on_free` — the only writers of the shadow — both open
with `if !is_enabled() { return; }`. The only thing that ever called `enable()`
outside a self-test was the `mm.corruption_hunt` block in `main.rs`, which arms
KASAN around one narrow region (the Path-Z spawn/teardown) and disarms it after.
Nothing tied `enable()` to the instrumented build.

The consequence is not "less coverage". It is *zero* coverage, delivered
silently:

| | ordinary build | instrumented build (before) | instrumented build (now) |
|---|---|---|---|
| Checks emitted per access | none | **every access** | every access |
| Shadow populated | only in the hunt window | **never** (unless the flag) | whole boot |
| Reports possible | in the hunt window | **none** | whole boot |
| Boot cost | 1x | **~3.5x** | ~3.5x + shadow upkeep |

The middle column is the one that matters: it is the only cell in the table
where the cost is paid in full and the benefit is exactly nil. And nothing in
the output said so — a boot with no findings looks identical to a boot that
cannot have findings. `scripts/kasan-build.sh`'s own header says the profile
"exists to root-cause heap corruption (B-KNULLJUMP) by having the compiler check
every load and store against the KASAN shadow", and it had never once done that.

### The alternatives

**A. Enable in the instrumented build, at init, for the whole boot.** *What
changes:* `kasan-build.sh --boot` now actually reports use-after-free and
redzone overruns anywhere in the boot, and takes longer than the 946 s it takes
today. Chosen.

**B. Leave the code alone; make `kasan-build.sh --boot` pass
`mm.corruption_hunt` on the cmdline.** *What changes:* the script's own runs
work; anyone who builds instrumented and boots by another route (`boot-test.sh
--no-build`, the soak harness, a manual QEMU line) still gets the silent no-op.
Rejected: it fixes one caller rather than the property, and the property is
exactly "you can hold this build wrong without being told". It would also leave
detection scoped to the Path-Z window, which is a *guess* about where the
corruption is — a guess the profile exists to stop having to make.

**C. Keep the flag, but have the instrumented build print a warning at boot when
KASAN is off.** *What changes:* the silence becomes audible, but the default is
still useless. Rejected as a strictly worse A: if the only correct way to run
the profile is with the flag, the profile should set the flag.

### The argument for A over "the cost is opt-in for a reason"

The counter-argument is that shadow upkeep on every allocation and free is
expensive and should be asked for. It does not survive contact with where the
cost actually is. In the instrumented build the *check* is already emitted at
every load and store — an outlined call, per `-asan-instrumentation-with-call-
threshold=0` — and that is the dominant cost by a wide margin. Shadow upkeep is
a handful of byte stores per alloc/free on top of it. Declining to pay the small
remainder buys nothing and forfeits the whole reason the large part was paid.

The flag keeps its meaning where the reasoning is different: in the *ordinary*
build there are no emitted checks at all, only the explicit `check()` calls on
the quarantine path, so there the cost genuinely is opt-in. Linux draws the line
in the same place — `CONFIG_KASAN` means KASAN is on, not that it is available.

### What this depends on, and what had to be fixed first

A only became safe once `disable()` wiped the shadow
(`B-KASAN-STALE-POISON-ON-LIVE-SLOT`, fixed the same day). Under A the boot
self-tests now find KASAN already enabled, so their `if !was_enabled { disable() }`
restore is a no-op, and the slot that `with_self_test_freed_address` frees stays
poisoned. That is *correct* under A and only under A: KASAN remains on, so
`on_alloc` clears the poison when the allocator hands the slot out again. Before
the wipe fix, the same sequence with KASAN ending up off is precisely what
produced 62 spurious use-after-free reports and exhausted the report budget two
thirds of the way through a boot. The two changes are not independent, and the
order matters.

It also depends on every path that hands out a heap slot running `on_alloc`.
Audited: `heap.rs` has exactly two alloc hook sites (the per-CPU fast path and
the global path, covering slab and large allocations alike) and one free hook,
and `quarantine_return_slot` moves in the free direction only — it returns a
parked slot to the slab still poisoned, which the next `on_alloc` clears. An
alloc path that skipped the hook would reintroduce the same false-UAF failure,
so this is the invariant to re-check if the allocator grows another entry point.

**That audit was right about the heap and wrong about its scope, which the first
whole-boot run demonstrated within 2092 lines.** Poison does not only outlive an
*object*; it outlives a *frame*. `large_dealloc` returns physical frames to the
buddy allocator carrying the `0xFA` that `on_free` just wrote, and the buddy
allocator reissues them to page tables, slab pages and DMA buffers — none of
which go anywhere near `on_alloc`. So the invariant is one level lower than
stated above: **every path that hands out memory must unpoison it, and the frame
allocator is such a path.** `frame::on_frames_allocated` is now the single hook
all five allocation return sites go through, and it unpoisons before the owner
tag so nothing can touch the frame first. See `known-issues.md` →
`B-KASAN-POISON-SURVIVES-FRAME-REUSE`.

**The first implementation of that hook was itself wrong, in a way worth
recording because the wrongness was invisible in review and obvious in the
stats.** Unpoisoning writes shadow bytes; writing a shadow byte normally *backs*
the shadow frame first; and backing it allocates a frame — so the hook re-entered
itself through the allocator that called it. The natural guard (a per-CPU flag
held across an interrupts-off window) does cut the recursion, but it forces every
call to reach `with_map_lock` with interrupts already off, and that is exactly
the case that gives up rather than spinning. A given-up *unpoison* fails
**closed**: the stale `0xFA` stays on live memory and every later access to it is
reported forever. The very first boot said so — `map_lock_giveups=3`, plus the
module's own warning that "shadow coverage has holes this boot" — in a build
where KASAN is live only for the self-test window. Whole-boot, firing on every
frame allocation, it would have manufactured the very false positives the hook
exists to remove.

The fix was to notice that the hook writes exactly one value, `KASAN_ADDRESSABLE`
(`0x00`), and that an *unbacked* shadow frame already reads `0x00` — the whole
shadow window resolves through a shared read-only zero page until something backs
it. So "back the frame, then write zero into it" and "do nothing" are the same
result, and the hook can simply skip unbacked frames (`IfUnmapped::Skip`). That
removes the allocation, and with it the recursion, the guard, the interrupts-off
window, the lock, and the hole — at zero cost, because the skipped write was a
no-op by construction. The general lesson: **an unpoison-only path never needs to
allocate shadow, and any design in which it does is buying a re-entrancy problem
for nothing.**

The same run also exposed `B-KASAN-SHADOW-BOOTSTRAP-SELF-REPORT`: `mm::kasan`'s
own `ensure_shadow_mapped` zeroed its freshly-allocated shadow frame with
`core::ptr::write_bytes`, an *instrumented* `core` generic (§118/§119), against
a frame that could itself be carrying stale poison. Turning KASAN on for the
whole boot is what made KASAN's own bookkeeping visible to KASAN.

That is the general shape of this entry's cost, and it is worth stating plainly
because it will recur: **A does not add bugs, it removes the condition — a
shadow nothing populates — under which existing ones were unobservable.** The
first whole-boot run should therefore be budgeted as a bug-finding exercise, not
a regression check, and a noisy first run is evidence the decision was right
rather than evidence against it.

### Cost accepted

Boot time rises above the measured 946 s by the shadow-upkeep margin, and the
shadow's lazily-mapped frames (one 16 KiB frame per 128 KiB of heap ever
touched) now cover the whole boot rather than one window. `KASAN_BOOT_TIMEOUT`
is 3600 s, ~3.8x the previously measured need, and was set generously for
exactly this kind of growth. If a future measurement puts a whole-boot
instrumented run near that ceiling, raise the constant rather than narrowing the
window — a narrowed window is the state this entry exists to leave behind.
