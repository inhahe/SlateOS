### TD-POSIX-TIMES-FLAKE. `posix::sys_times::tests::test_times_increments_each_call` fails intermittently under a full-workspace run — 2026-08-15 — ✅ FIXED 2026-08-15 by lane B (`posix/src/sys_times.rs`), and the triage found a second, worse bug underneath it

**Status: FIXED 2026-08-15** (lane B, `posix/src/sys_times.rs`). Stays here
rather than moving to `known-issues-resolved.md` until the fix has been on
`main` through a full boot test.

**Not lane C's tree** — logged here so the next lane to hit it does not re-triage
it from scratch. `posix/src/sys_times.rs:583-595` asserts
`assert_eq!(t, base + i, "each call should increment by 1")` against what the
module doc itself describes as a **process-wide** monotonic call counter, inside
a `-p posix --lib` binary that runs **20289 tests across parallel threads**. Any
concurrent `times()` caller bumps the shared counter between this test's own
calls, so the exact-delta premise holds only by scheduling accident.

**Symptom.** `cargo test --workspace --target x86_64-pc-windows-gnu` (674 s)
came back `20288 passed; 1 failed` with `left: 8`. Re-running the same binary
filtered (`-- sys_times`, 25 tests) passes every time — so it reproduces *only*
in the full-workspace run, which is precisely the run every lane must make green
before merging up. It therefore looks like a red tree caused by whoever is
merging, and costs that lane a triage cycle to prove otherwise.

**Correct fix** (lane B's to make): assert strict monotonicity (`t > prev`),
which is what the implementation actually guarantees. Serialising the
`times()`-using tests behind a `Mutex` also works but is more fragile — it holds
only as long as every future `times()` caller in the crate remembers the lock.

**Superseded in part** — see `B-POSIX-SYS-TIMES-HOST-STUB-STATIC-MUT-DATA-RACE`
directly below. The "correct fix" above is *not* sufficient: the counter is a
racy `static mut`, so strict monotonicity does not hold either, and a second
test that already asserts exactly `t > prev` fails for that reason.

**✅ FIXED 2026-08-15 (lane B).** Lane C's diagnosis and preferred fix were both
right, and taking it seriously turned up a second bug that the flake was sitting
on top of.

*The reported half.* `test_times_increments_each_call` is now
`test_times_is_strictly_monotonic` and asserts `t > prev`, which is the
counter's real contract. Its doc comment carries the whole history so nobody
"tightens" it back into an exact-delta assertion; the module doc that seeded the
mistake ("a monotonic call counter (for unit-test determinism)") now says
*process-wide* and *strictly increasing* in as many words, and states outright
that `t2 == t1 + 1` is not assertable.

*The half that was not in the report.* (Written before merging `origin/main`,
where lane A had already filed this same race — see the entry directly below
and the process note attached to it.) The counter was a `static mut TICK_COUNTER: i64`
bumped by a plain read-modify-write under the comment `// SAFETY:
single-threaded access` — in a binary that runs 20 289 tests across a thread
pool. That comment was simply false, and the race is undefined behaviour, not
merely non-deterministic: two callers can read the same pre-increment value and
return the **same** tick. That would have broken the strict-monotonicity
assertion too, i.e. lane C's suggested fix would have been flaky for a second,
much less obvious reason. It is now an `AtomicI64` with
`fetch_add(1, Relaxed)`, which makes every caller's value distinct and totally
ordered; `Relaxed` is sufficient because single-location coherence already
guarantees a thread's own successive calls increase, and nothing else is
published through the counter.

*Regression cover.* `test_times_hands_out_distinct_ticks_across_threads` runs 8
threads × 200 calls, released together through a bounded spin barrier, and
asserts every returned tick is distinct. The barrier is load-bearing: without
it the first thread finishes before the last is spawned, and a racy
implementation would pass vacuously. 26/26 `sys_times` tests green.

*Why this was worth the extra step.* The reported symptom was "a test asserts
more than the implementation promises." The actual state was "the
implementation does not reliably deliver even what the weaker assertion
checks." Fixing only the test would have left a UB data race in a libc
primitive, still green, and made the next failure look like a fresh flake.
