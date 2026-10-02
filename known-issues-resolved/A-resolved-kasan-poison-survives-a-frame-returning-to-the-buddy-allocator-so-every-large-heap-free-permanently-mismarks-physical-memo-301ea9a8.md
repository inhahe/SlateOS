### [A] RESOLVED — KASAN poison survives a frame returning to the buddy allocator, so every large heap free permanently mismarks physical memory that is then reused for anything at all — 2026-08-19

**Id:** `B-KASAN-POISON-SURVIVES-FRAME-REUSE`

**Resolved 2026-08-19** by hooking the frame allocator (`frame::on_frames_allocated`
→ `kasan::on_frame_alloc`) on all five allocation return sites. Measured on a
whole-boot instrumented run (`scripts/kasan-build.sh --boot`, `BOOT_OK` in 937 s
of a 3600 s budget, boot test PASSED):

| | Before | After |
|---|---|---|
| KASAN reports | **64** by serial line 2092 (budget exhausted) | **3** in 27 592 lines |
| of which false positives | 64 | **0** |
| `map_lock_giveups` | 3, with "shadow coverage has holes this boot" | **0** |
| shadow coverage | — | 2630 frames; 1 496 684 080 B poisoned / 4 294 571 020 B unpoisoned |

All three surviving reports are *deliberate* self-test violations, and each is
matched by the subsystem's own detector agreeing with KASAN: two from
`kasan-rt`'s report-path test, and one from the heap slab-poison test's
intentional overflow — `[kasan] … out-of-bounds (heap redzone) on write of 1
bytes @ 0xffff800131b5c32c` against `[heap] BUFFER OVERFLOW detected!
slot=0xffff800131b5c300, alloc=40, class=64, offset=44` (`0xc32c` = slot + 44).
So the profile is now both quiet *and* demonstrably live — the failure shape
called out at the bottom of this entry (a check that cannot fire, presenting as
a check that found nothing) is excluded by the same evidence that shows it clean.

Note that these three deliberate violations consume 3 of the 64-report budget on
every instrumented boot. That is left as-is deliberately: the cost is negligible
and the reports are positive evidence each boot that the detector still works.

**In short:** KASAN marks memory "freed" when the heap frees it, and "usable"
again when the heap hands it out. But *large* heap allocations do not come from
the heap's own slabs — they come straight from the physical frame allocator, and
freeing one gives those physical frames back to the frame allocator, which will
happily reuse them for something that is not a heap object at all: a page table,
a slab page, a DMA buffer. Nothing on those paths tells KASAN the memory is
usable again, so the "freed" mark stays. From then on every legitimate access to
that memory is reported as a use-after-free. Turning KASAN on for the whole boot
(§233) surfaced this instantly: 64 reports — the entire per-boot report budget —
inside the first 2092 lines of boot, after which real findings are suppressed.

#### Evidence

First instrumented boot with KASAN active from init:

```
499:[kasan] CRITICAL: use-after-free (freed heap) on write of 8 bytes @ 0xffff80007fe4f000
513:[kasan] CRITICAL: use-after-free (freed heap) on write of 8 bytes @ 0xffff80007fe4e000
527:[kasan] CRITICAL: use-after-free (freed heap) on write of 8 bytes @ 0xffff80007fe4d000
541:[kasan] CRITICAL: use-after-free (freed heap) on write of 8 bytes @ 0xffff80007fe4c000
555:[kasan] CRITICAL: use-after-free (freed heap) on read  of 8 bytes @ 0xffff80007fe4c000
586:[kasan] CRITICAL: use-after-free (freed heap) on read  of 8 bytes @ 0xffff80007fe4c008
604:[kasan] CRITICAL: use-after-free (freed heap) on read  of 8 bytes @ 0xffff80007fe4c010
...
```

Three things identify this as stale poison rather than a real use-after-free:

1. **The write addresses are 4 KiB apart and descending** — `…4f000`, `…4e000`,
   `…4d000`, `…4c000` — which is a page walk, not an object.
2. **All four share one identical 12-frame backtrace**, so they are one loop.
3. **The reads that follow march `+0, +8, +0x10, +0x18, …` from the lowest of
   them**, which is a linear scan of freshly acquired memory, i.e. exactly what a
   *new owner* of a reused frame does.

`0xffff80007fe4c000` is HHDM + `0x7fe4c000` ≈ 2.14 GiB into a 3 GiB guest — a
direct-map alias of a physical frame, not a slab slot.

#### The mechanism

| Step | Code | Shadow effect |
|---|---|---|
| large alloc | `HeapInner::large_alloc` → `frame::alloc_order` → `to_virt(hhdm)` | `on_alloc` (heap.rs:1300) unpoisons |
| large free | `GlobalAlloc::dealloc` → `on_free` (heap.rs:1332) | **whole region poisoned `0xFA`** |
| ...then | `HeapInner::large_dealloc` → `frame::free_order` | frames go to the buddy allocator |
| reuse | `frame::alloc_frame` for a page table / slab page / DMA buffer | **nothing unpoisons** |
| any access | via the HHDM alias | reported as use-after-free, forever |

This is `B-KASAN-STALE-POISON-ON-LIVE-SLOT` one layer down, and the same shape:
*poison outliving the object because reuse happens through a path that does not
unpoison.* The earlier fix put the teardown on `disable()`, which handles the
enable/disable transition; it cannot help here, because KASAN never gets
disabled — the poison outlives the *allocation*, not the *enabled window*.

It has always been latent. It was invisible before §233 only because KASAN was
never enabled long enough for a large free and a frame reuse to both land inside
the same enabled window.

#### The fix (as applied)

**Hook the frame allocator, which is what Linux does** (`kasan_unpoison_pages()`
on page alloc, `kasan_poison_pages()` on page free). Concretely:

1. Add `kasan::on_frame_alloc(phys, bytes)` and call it from `frame::alloc_frame`
   and `frame::alloc_order`, unpoisoning the HHDM alias of the returned frames
   **before the allocator or its caller touches them** — a frame that is zeroed
   through HHDM on the way out would otherwise take an instrumented access to
   still-poisoned memory and report.
2. That alone is a correctness fix and fails open: after it, no stale poison can
   survive frame reuse regardless of what the frame becomes.

**Re-entrancy is the trap to get right, and the first answer to it was wrong.**
`unpoison_range` → `set_shadow` → `ensure_shadow_mapped` → `frame::alloc_frame`
→ the new hook → `unpoison_range`… The shadow VA itself is outside the covered
window so it does not recurse, but the *HHDM alias of a freshly allocated shadow
frame* is inside it, and `ensure_shadow_mapped` zeroes exactly that. Each level
covers 8x more memory so it terminates in practice, but "terminates in practice"
is not the standard this module holds itself to (see the raw-accessor
termination argument at the top of `kasan.rs`).

The prescription originally written here — *"use an explicit per-CPU re-entrancy
flag and skip the hook when already inside it"* — was implemented and is
**wrong**. The flag is only sound with interrupts disabled (otherwise an
IRQ-context allocation on the same CPU sees the flag set and skips its own
unpoison), and disabling interrupts means every call reaches `with_map_lock` on
its `!irqs_were_on` path — the one that **gives up instead of spinning**. A
given-up unpoison fails *closed*: the stale `0xFA` stays on live memory and every
later access to it is reported forever. That is this very bug, reintroduced by
its own fix. The first boot with that version said so plainly:
`map_lock_giveups=3` and `WARNING: 3 IRQ-context allocation(s) went unpoisoned
(MAP_LOCK busy) — shadow coverage has holes this boot`, in a build where KASAN is
live only for the self-test window. Whole-boot, firing on every frame allocation,
it would have been far more.

**The correct answer is not to guard the recursion but to not have it.** The hook
writes exactly one value, `KASAN_ADDRESSABLE` (`0x00`), and an *unbacked* shadow
frame already reads `0x00` — the entire shadow window resolves through a shared
read-only zero page until something backs it (`ZERO_PT`'s 512 entries all point
at one all-zero page; `get_shadow` likewise returns `KASAN_ADDRESSABLE` for an
unmapped frame). So "back the shadow frame, then write zero into it" and "leave
it alone" are the same result. `fill_shadow_with(..., IfUnmapped::Skip)` takes
the second, which allocates nothing — no recursion, no guard, no interrupts-off
window, no `MAP_LOCK`, no hole — at zero cost, because the skipped write was a
no-op by construction. The fix still lands where it matters: poisoning *does*
back the shadow frame, so any frame carrying stale `0xFA` necessarily has its
shadow bit set and gets cleared.

Generalised: **an unpoison-only path never needs to allocate shadow, and any
design in which it does is buying a re-entrancy problem for nothing.**

**Deliberately not doing (yet):** poisoning on frame *free*. That would add
page-granularity use-after-free detection, which is a genuinely stronger check
and attractive — but it is a new claim rather than the removal of a false one,
and it risks its own false positives (the frame allocator writes its own
metadata and freelist links through the HHDM alias of frames it has just
freed). Land the unpoison side, get a clean whole-boot instrumented run, and
consider poisoning-on-free as a separate change with its own boot evidence.

> **ANNOTATION 2026-08-19 (lane A): done — the trigger above was met, and
> poison-on-free has landed.**
>
> Both halves of the condition were satisfied on their own terms:
>
> - **The clean whole-boot instrumented run exists.** `./scripts/kasan-build.sh
>   --boot` reached `BOOT_OK after 1251s` across 27582 serial lines, `build:
>   kasan-instrumented on QEMU TCG`, with `violations=7, shadow_frames=2625,
>   poisoned=1496682224B, unpoisoned=4294422947B, map_lock_giveups=0`. There
>   were exactly **three** `CRITICAL` lines in the entire boot (serial lines
>   25627, 25636, 27123) and all three trace to deliberate self-tests — the
>   `[kasan-rt]` report-path test and the `[heap]` buffer-overflow/redzone
>   test. **Zero spurious reports.** Compare the pre-fix run, which exhausted
>   its 64-report budget by line 2092.
> - **The false-positive risk the paragraph feared was already defused, not
>   merely tolerated.** `kernel/src/mm/frame.rs` carries a module-scope
>   `#![cfg_attr(kasan_instrumented, sanitize(address = "off"))]` (line 61), so
>   the buddy allocator's intrusive `FreeNode` freelist writes through the HHDM
>   alias, and its zero-on-free memset, are not checked at all. The metadata
>   the paragraph worried about cannot report against itself.
>
> What landed: `kasan::on_frame_free`, called from `free_frame`'s per-CPU fast
> path and from `free_order_inner`, gated on a new shared `block_is_sole_owned`
> predicate (CoW frames with refcount > 1 must not be poisoned — the same
> predicate zero-on-free applies, now hoisted out of the `is_zero_on_free()`
> gate so the two are independently switchable). It writes `KASAN_FREE` with a
> new `IfUnmapped::SkipLossy`, which skips ranges whose shadow is unbacked
> rather than allocating shadow from inside the free path. See
> design-decisions.md §244 for why dropping a poison is acceptable where
> dropping an unpoison was not: poison fails **open** (a missed report on a
> frame that was never watched anyway), unpoison fails **closed** (stale `0xFA`
> on live memory, reported forever).
>
> Covered by self-test 6 in `kasan::self_test`, which forces the frame's shadow
> to be backed (otherwise `SkipLossy` would drop the poison and the assertion
> would fail for a reason unrelated to the hook), then asserts the frame reads
> `KASAN_FREE` at both ends after `free_frame` and reads addressable again if
> the allocator hands the same frame back.
>
> **Post-change instrumented evidence.** A second full `kasan-build.sh --boot`,
> *with* the hook, also passed: `BOOT_OK after 1464s`, 27588 serial lines,
> `build: kasan-instrumented on QEMU TCG`, clean streak 12. Exactly the **same
> three** `CRITICAL` lines as the pre-change baseline, all deliberate
> self-tests — the two `[kasan-rt]` report-path writes at 25638/25647 and the
> `[heap]` redzone overflow at 27135 — and `map_lock_giveups=0`. **The new hook
> introduced zero false positives.** That the hook is doing real work rather
> than silently no-opping is visible in the stats line: `poisoned` went from
> 1496682224B to **3964078784B**, i.e. ~2.47 GB of additional poison, ~151k
> frames poisoned on free. `unpoisoned` is essentially unchanged
> (4294422947B → 4294639048B), as it must be — the alloc side was not touched.
>
> The delta is the right shape for a *sanity* check on the numbers, not just an
> assertion that they went up: if `SkipLossy` were dropping most poisons the
> figure would have barely moved, and if the sole-owner predicate were wrong in
> the permissive direction it would have exceeded the unpoisoned total.
>
> **Benchmark evidence, and a trap worth writing down.** `frame.rs` is
> performance-critical, so the harness required `--bench`. The first run looked
> alarming — 18 benchmarks "REGRESSED, UNCONFIRMED", including
> `page_alloc_zeroed_pool` +65% and `heap_alloc_free_64` +74%. None of it
> survived contact with the data.
>
> *First trap: two accelerator populations.* The script passes no `-accel`, so
> QEMU takes WHPX when the host offers it and silently falls back to TCG when it
> does not — the accelerator flips on host state, not on anything in the
> command. All 13 "fast" historical runs are Hyper-V/WHPX; mine are TCG. The
> tell is `page_alloc_zeroed_free`: 457–542 ns across every WHPX run and
> **3734 ns** on `26c139a81`, which is a *pre-change* run and the only other TCG
> one. My runs sit at 3738/3621/3583 — identical to that TCG baseline. The
> harness does pick an accel-matched baseline for the per-benchmark comparison
> (it chose `26c139a81`), so this is not a harness defect; the problem is that
> the matched population had exactly **one** member, so no spread was known.
>
> *Second trap: the TCG noise floor is wider than the effect being chased.*
> Three A/A runs on the byte-identical image (`sha:2e6316daa443fc8e`) settled
> it, and the two benchmarks that had looked persistent moved on their own:
>
> | benchmark | TCG baseline (pre-change) | run 1 | run 2 | run 3 |
> |---|---|---|---|---|
> | `page_alloc_free` | 461 | 508 | 370 | 540 |
> | `page_alloc_zeroed_free` | 3734 | 3738 | 3621 | 3583 |
> | `page_alloc_zeroed_pool` | 348 | 611 | 742 | **349** |
> | `firewall_check` | 66 | 96 | 76 | **65** |
> | `heap_alloc_free_64` | 186 | 345 | 378 | 497 |
>
> `page_alloc_zeroed_pool` and `firewall_check` returned to within 1 ns of the
> pre-change baseline, and the harness's own A/A verdict names
> `page_alloc_free: 370 → 540 (+45%)` and `heap_alloc_free_64: 378 → 497 (+31%)`
> as *measured host noise, by construction* — the same two benchmarks the first
> run had flagged. `firewall_check` was the giveaway throughout: nothing in this
> change can reach a firewall rule match, so its +45% in run 1 could only ever
> have been a whole-run factor.
>
> Conclusion: no regression. `page_alloc_free` PASSed its 1000 ns target on all
> three runs, `page_alloc_zeroed_free` is flat, and the code-level cost is
> bounded by inspection at one `#[inline]` call whose first act is a relaxed
> atomic load that is `false` in production, plus two more relaxed loads
> short-circuiting a volatile read in `free_order_inner`.
>
> **The reusable lesson:** under TCG on this host the per-benchmark noise floor
> exceeds ±45%, so a TCG-only comparison against a single baseline grades
> nothing. Before believing a `--bench` regression, check `accel` in
> `bench/history.jsonl` and confirm the baseline population is both
> accel-matched *and* larger than one. A benchmark the change provably cannot
> reach (`firewall_check` here) is the cheapest available control — if it moved
> too, the run moved, not the code.

#### Why this matters more than the report count suggests

The 64-report budget is exhausted at line 2092 of a ~27700-line boot. Everything
after that is unprotected, so the profile is at present *worse* than useless for
its stated purpose (`B-KNULLJUMP`): it consumes its entire detection budget on
false positives in the first 8% of the boot and then goes quiet, which reads
exactly like a clean run. That is the same failure shape as the two entries above
it — a check that cannot fire, presenting as a check that found nothing.
