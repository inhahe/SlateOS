### B-FTS-INSTANCE-POOL-IS-SHARED-ACROSS-THREADS. Two concurrent `fts_open` calls could be handed the *same* stream — 2026-08-13 — FIXED 2026-08-13

**Where:** `posix/src/fts.rs`, `FTS_INSTANCES` / `FTS_HANDLES` and the
`allocate_instance` / `release_instance` / `with_instance` helpers.

**Symptom that exposed it:** `cargo test --workspace` intermittently failed
`fts::tests::test_fts_open_close_roundtrip` with `fts_close` returning `-1`
instead of `0`. The test passed in isolation and only failed with more than one
test thread, which is the classic signature of a shared-global race rather than
a test bug.

**Root cause — the pool was in the wrong storage class.** The slot table was a
bare `static mut`, and occupancy was a plain `bool` field claimed by a
check-then-set:

```rust
static mut FTS_INSTANCES: [Instance; MAX_FTS_INSTANCES] = ...;   // WRONG scope
...
for (i, inst) in table.iter_mut().enumerate() {
    if !inst.in_use {          // check ...
        *inst = INSTANCE_INIT;
        inst.in_use = true;    // ... then set
        return Some(i);
    }
}
```

Every *other* slot pool in the crate (`epoll`, `dirent`, `stdio`, `mqueue`,
`aio`, `semaphore`, the three `sysv_*`) declares its table with the
`process_global!` macro from `posix/src/perprocess.rs`. That macro is a plain
`static mut` on the target but a `thread_local!` on the host, precisely so that
one host test thread's fd/handle table is not another's — `perprocess.rs` even
carries a test (`writes_are_not_shared_between_threads`) for exactly this
failure mode. `fts.rs` was the one pool that never adopted it, so its two slots
were shared by every test thread: two threads could observe the same free slot,
both claim it, then interleave writes into one `Instance` (same traversal stack,
same path buffer), and whichever called `fts_close` second found the slot
already released and returned `EBADF`. The visible test failure was the
*mildest* possible outcome; the silent one is two `fts` walks corrupting each
other's state.

**Fix:** `FTS_INSTANCES` and `FTS_HANDLES` moved into `process_global!`
(`instances_ptr()` / `handles_ptr()`), making `fts` consistent with all eleven
sibling pools. The handle bodies had to move too — an `FTS *` is only meaningful
against its own process's slots.

A bespoke `AtomicBool` claim bitmap was written first and then backed out: it
would have made `fts` the only pool in the crate with a different concurrency
model while leaving the other eleven untouched, which is worse to maintain and
fixes nothing the storage-class change doesn't. The residual question — whether
the target build's "one single-threaded process" assumption behind
`process_global!` still holds now that the crate implements `pthread_create` —
is a crate-wide design issue and is tracked separately as
`TD-POSIX-SLOT-POOLS-ASSUME-A-SINGLE-THREADED-PROCESS`.

**Follow-on:** with the pool per-thread, two tests could be tightened from
"either outcome is acceptable" into real assertions —
`test_fts_open_close_roundtrip` now requires the open to succeed (and re-opens
`3 * MAX_FTS_INSTANCES` times to prove released slots return to the pool), and
`test_fts_open_exhausts_pool_returns_enomem` now requires exactly
`MAX_FTS_INSTANCES` successes, distinct handles, then `ENOMEM`. Both previously
tolerated a null return "because parallel tests may have taken the slots",
which is what let the shared-table bug hide.

**Lesson:** a tolerant assertion written to paper over test flakiness will hide
the very bug it was papering over. When a test says "either result is fine
because of parallelism", that is evidence of a shared-state defect, not a
reason to weaken the assertion.
