### B-POSIX-ATEXIT-TESTS-RACE-ON-PROCESS-GLOBAL-COUNTERS. Five `posix` unit tests fail intermittently under `cargo test --workspace` — 2026-08-13 — ✅ FIXED 2026-08-13 (`posix/src/crt.rs`)

**Symptom.** `cargo test --workspace` failed with five failures in
`posix::crt::tests`, with nonsense expectations:

```
test_at_quick_exit_multiple_registrations: left: 5, right: 2
test_atexit_table_full:                    left: 0, right: -1
test_atexit_and_quick_exit_separate:       left: 2, right: 1
```

Re-running the same tests with `--test-threads=1` passed 72/72.

**Cause.** Seven tests exercise `atexit` / `at_quick_exit`, which register into
the *process-global* `ATEXIT_COUNT` / `QUICKEXIT_COUNT`. Each test works by
writing a starting count, calling the registration function, and asserting the
resulting count. Cargo runs a thread per test by default, so two of them
interleave and trample each other's setup — `test_atexit_table_full` writes
`MAX_ATEXIT` and expects the next `atexit` to return `-1`, but a neighbouring
test's cleanup writes 0 in between, so it returns 0.

Note the count of *five* failures out of seven: the race is timing-dependent, so
this suite was intermittently green and had presumably been passing often enough
not to be noticed. Discovered while running the workspace suite as a regression
check for the SMAP work — unrelated to it.

**Fix.** An `ATEXIT_TEST_LOCK` mutex held for the whole body of all seven tests
(reset *and* cleanup, since the reset is itself a mutation of the shared state).
The same pattern `INIT_ARRAY_TEST_LOCK` already uses a few hundred lines above
for `.init_array` ordering, which has the identical problem. The guard is taken
through a helper that recovers from poisoning rather than `expect`ing: a genuine
assertion failure panics while holding the lock, and propagating the poison would
turn one real failure into six "test lock poisoned" ones that hide it.

Verified by eight consecutive parallel runs, all 72/72, plus a clean
`cargo test --workspace`.

**Lesson for the rest of the tree.** Any `#[test]` that writes a `static mut` is
suspect. The failure is invisible when the suite happens to schedule the tests
apart, so "it passed last time" is not evidence. When adding a test that touches
process-global state, add it to the existing lock rather than assuming the reset
at the top of the test is sufficient — it is not; the reset is part of the race.
