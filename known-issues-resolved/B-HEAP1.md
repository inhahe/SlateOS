### B-HEAP1. Kernel heap redzone "overflow" reports during init file-install were FALSE POSITIVES from a pre-poison allocation window — FIXED 2026-06-16

**Symptom (as originally observed):** During boot (init step 24, after all
self-tests), the debug heap allocator's dealloc-time redzone scanner reported
several `[heap] BUFFER OVERFLOW detected! slot=…, alloc=N, class=C, offset=N`
lines, e.g. `alloc=10, class=16, offset=10` (right before
`[init] Installed /bin/hello`) and two `alloc=18, class=32, offset=18`. Boot
still reached `BOOT_OK` and all self-tests passed.

**Root cause (NOT a real overflow):** The redzone check relies on the invariant
"every byte in `[alloc_size, class_size)` is `ALLOC_POISON` (0xCD)". That holds
only if the slot was `poison_alloc`'d *at the time it was handed out*. But
`enable_poison()` was called very late in boot (`kernel/src/main.rs` step 22f-3,
old line ~3518) while the heap is initialized far earlier (`mm::heap::init`,
~line 455). **Every allocation made in that window was never poison-filled.**
When such a slot was later freed *after* poisoning came online, `check_redzone`
scanned whatever bytes the pre-poison occupant had left there — zeroed
fresh-frame bytes, or stale content from an earlier reuse — and reported them as
overflow. Captured byte dumps confirmed this: a slot freed with `alloc_size=18`
held the intact 31-char string `/tmp/tmpwatch_test/delete_me.tmp` filling the
whole 32-byte class (a former occupant), and `"/bin/hello"+'e'+zeros` showed
unpoisoned (zero) redzone bytes — neither is possible if the slot had actually
been alloc-poisoned. So the reports were detector false positives, not memory
corruption.

**Fix:** Move `mm::heap::enable_poison()` to immediately after `mm::heap::init()`
(`kernel/src/main.rs`, step 6), *before the first heap allocation*. With no
pre-poison allocation window, every slab slot is poison-filled at its first
alloc and the redzone invariant always holds. The redundant late
`enable_poison()` at step 22f-3 was removed (the `poison_self_test()` call
stays). Poison is still toggled OFF only for the duration of the heap
benchmarks (`deferred_bench_task`), which free their own allocations within that
window. Note this only affects slab classes (≤ 8192 B); large allocations (the
actual MB-sized binaries) go through the buddy path and are never poisoned or
redzone-checked, so the early-enable adds negligible boot cost.
