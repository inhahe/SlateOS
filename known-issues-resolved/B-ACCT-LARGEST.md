### B-ACCT-LARGEST. `accounting` self-test "Largest RSS" assumed test-only isolation, panicking when a live process held >50 RSS frames — FIXED 2026-06-30

**Where:** `kernel/src/mm/accounting.rs`, self-test "Largest RSS"
section (was ~line 507). The test charged two fake PML4s (a=20, b=50)
then asserted `largest_rss().pml4_phys == pml4_b`. But `largest_rss()`
scans the **global** accounting table, which during a live boot also
contains *real* process address spaces. Whenever a concurrent real
process happened to hold >50 frames at that instant, `largest` was that
real PML4 (e.g. `0x1DFE0000`, not the fake `0xBEEF0000`), so the
`assert_eq!` panicked and **hard-halted the whole boot**, masking every
self-test after it. A load-dependent flake: it passed on light boots
and failed under heavier ones.

**Fix:** the assertion was false-isolation; replaced with invariants
that hold deterministically even with real entries present:
(1) among the test's own entries, `query` confirms b (50) outranks
a (20); (2) `largest_rss().rss_frames >= 50` — i.e. it returns a true
global upper bound — instead of asserting it equals a specific fake
PML4. Verified: clean build + green boot self-test.
