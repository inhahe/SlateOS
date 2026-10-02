## B-TWO-POSIX-TDESTROY-TESTS-SHARE-ONE-COUNTER (found by lane C, 2026-08-22 — lane B's crate) — FIXED 2026-08-22

**In short:** Two tests in `posix/src/search.rs` reset and read the same
process-wide counter, and `cargo test` runs them on different threads at the
same time. When one's reset lands in the middle of the other's measurement, the
other reads zero and fails. Nothing is wrong with `tdestroy`.

**Where.** `posix/src/search.rs:919` declares
`static DESTROY_COUNT: AtomicI32`; `test_tdestroy_empty` (`:929`) and
`test_tdestroy_calls_free_fn` (`:944`) both `store(0)` into it and both read it.

**The observed failure is exactly the predicted interleaving:**

```
thread 'search::tests::test_tdestroy_calls_free_fn' panicked at posix\src\search.rs:946:9:
assertion `left == right` failed: tdestroy should call free_fn for each node
  left: 0
 right: 5
```

`left: 0` is what you get when `test_tdestroy_empty`'s `store(0)` lands *after*
the five `fetch_add`s and before the `load`.

**Fix.** One static and one `extern "C"` callback per test — two lines, no
synchronisation and no ordering assumption. A `Mutex` around both would also
work and is worse, because it makes each test depend on its relationship to the
other, which is the thing that just bit. `test_tdestroy_empty` in fact needs no
counter at all: it asserts *no* calls, so a callback that panics on entry is a
stronger assertion.

**Filed to the owning lane** in
`requests/c-b-three-flaky-tests-fail-the-workspace-gate.md`. Lane C has not
touched the file.

**How it was found.** The same `cargo test --workspace --no-fail-fast` run that
turned up the `ftpd` half of
`B-POLKIT-FAILLOCK-TEST-RACES-ITS-OWN-ONE-SECOND-DELAY`. Two consecutive full
runs failed on *different* subsets of the three, which is what identified all of
them as load-sensitive races rather than regressions.

### Fixed 2026-08-22 (lane B) — thread-local counters, not per-test statics

`DESTROY_COUNT` and `WALK_COUNT` are now `std::thread_local!` `Cell<i32>`s with
`reset_*`/`*_count()` accessors, so each test observes only its own thread's
count. `libtest` gives every test its own thread, so that is exactly per-test
isolation — no lock, no serialisation, and the tests still run concurrently.
It is the same shape, and the same argument, as `malloc::live_regions`, which
had already solved this problem one file away.

**Two deviations from lane C's prescription, both deliberate:**

1. *"One static and one callback per test"* fixes the two `tdestroy` tests and
   leaves `WALK_COUNT` alone — but `WALK_COUNT` has the identical defect and
   **three** tests on it (`test_twalk_empty`, `test_twalk_single`,
   `test_twalk_multiple`), each doing the same store-walk-load. Per-test
   duplication would mean five statics and five callbacks, and would leave the
   next test added to either group to rediscover the rule. Thread-locals fix
   both counters at once and make the correct thing the default.
2. *"`test_tdestroy_empty` needs no counter — a callback that panics on entry is
   a stronger assertion"* is a good idea that is unsafe **here**: the callback
   is an `extern "C"` function, and a panic unwinding out of an `extern "C"`
   frame aborts the process rather than failing the test. That would convert a
   test failure into a dead test binary — precisely the failure mode being
   fixed in the sibling entry below. With a thread-local counter, reading `0`
   is already a real assertion about this test alone, so the counter is doing
   the job the panic was meant to do.
