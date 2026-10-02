### B-COMPACT1. Memory-compaction self-test (`collect_private_frames`) panicked non-deterministically across boots — FIXED 2026-06-16

**Where:** `kernel/src/mm/compact.rs` — `self_test()` Test 5; the API under test is
`kernel/src/mm/rmap.rs::collect_private_frames`.

**What it was:** the self-test added one fake private rmap entry, then called
`collect_private_frames(&mut [0u64; 4], 0)` once and asserted the fake frame was
among the (up to 4) results. `collect_private_frames` fills its `out` buffer with
the first `out.len()` private frames in table-index order, starting from the cursor
and wrapping once around the whole 16384-slot table. By the time the compaction
self-test runs, the rmap table already holds entries from other subsystems (a
failing boot showed ~16). With a 4-slot buffer, only the four lowest-indexed
private frames are returned; whether the fake entry (hashed to slot
`0x0F00_0000 % 16384`) is among them depends on what else occupies lower slots —
so the assertion passed or panicked depending on unrelated boot state. The panic
(`"collect_private_frames should find our fake entry"`) aborted the kernel mid-boot,
failing the Path-Z boot-test.

**Fix (2026-06-16):** the test now pages through the table with a 32-slot buffer
(larger than the live entry count, so a single full sweep already finds every
private frame including the fake one) and a bounded loop that advances the
continuation cursor each page, breaking as soon as the fake frame is seen, the
table is exhausted (`found == 0`), the cursor stops advancing, or a 64-page hard
cap is hit (guaranteed termination). This makes the test deterministic regardless
of how many unrelated rmap entries exist. Verified: BOOT_OK with
`[compact]   collect_private_frames: OK (saw_fake=true)` and
`[compact] Self-test PASSED`, 0 self-test failures.

**Related debt (not fixed):** `collect_private_frames`'s continuation/pagination
is mildly broken as a "visit every unique private frame exactly once" iterator —
each call performs a *full* `0..RMAP_TABLE_SIZE` sweep from `start_idx`, so when
more than `out.len()` private frames exist the continuation re-encounters frames
below the cursor on the next page (it never returns the `(found, 0)` "scan
complete" sentinel). The production consumer
(`compact.rs::try_compact`, 4 batches × 32) tolerates this — it re-checks each
candidate via `try_migrate_one` and only wastes a little work re-examining
duplicates — so it is a performance/clarity wart, not a correctness bug. A proper
fix would have the continuation scan only the *remaining* `[next, original_start)`
window rather than re-sweeping the whole table. Tracked here; low priority.
