### B-POSIX-SYS-TIMES-HOST-STUB-STATIC-MUT-DATA-RACE. The host `times()` tick counter is an unsynchronised `static mut` — it goes *backwards* under `cargo test`, so it is not even monotonic — 2026-08-15 — ✅ FIXED 2026-08-15 by lane B (`posix/src/sys_times.rs`), same commit as `TD-POSIX-TIMES-FLAKE` above; originally filed as `requests/a-b-sys-times-host-stub-static-mut-data-race.md`

**Status: FIXED 2026-08-15** (lane B, `posix/src/sys_times.rs`, same commit as
`TD-POSIX-TIMES-FLAKE` above). Stays here until the fix has been on `main`
through a full boot test.

**Not lane A's tree** — logged here so this is not re-triaged from scratch, and
because it changes the recommended fix for `TD-POSIX-TIMES-FLAKE` above.

**What it is.** `posix/src/sys_times.rs:85` declares
`static mut TICK_COUNTER: i64 = 0`, and lines 195-200 do an unsynchronised
read-modify-write on it under the comment `// SAFETY: single-threaded access.`
That comment is false: `cargo test` runs tests in parallel threads within one
binary, so this is a data race on a `static mut` — undefined behaviour, not
merely a lost update.

**Symptom.** Lane A's pre-merge `cargo test --workspace --target
x86_64-pc-windows-gnu` (906 s) came back `20288 passed; 1 failed`:

```
thread 'sys_times::tests::test_times_null_buffer_loop_phase154' (31140)
panicked at posix\src\sys_times.rs:468:13:
iteration 106: tick count must advance (prev=172, cur=161)
```

The counter went **backwards**, 172 → 161 — the exact signature of a lost
update (T1 read 172 and wrote 173; T2, holding a stale 160, wrote 161; T1 then
read 161).

**Why this matters beyond one test.** `TD-POSIX-TIMES-FLAKE` recommends
replacing the exact-delta assertion with a strict-monotonicity one, "which is
what the implementation actually guarantees." It does not guarantee that.
`test_times_null_buffer_loop_phase154` *already* asserts precisely
`assert!(cur > prev)` — and that is the test that failed. Adopting the
recommended fix would turn a test that fails often into a test that fails less
often while leaving the UB in place.

**Correct fix** (lane B's to make): replace the `static mut` with
`AtomicI64` and return `fetch_add(1, Relaxed) + 1`. That removes the UB, makes
strict monotonicity a real per-location property rather than a scheduling
accident, and needs no `unsafe` — so the false SAFETY comment disappears with
it. Lane C's fix to `test_times_increments_each_call` is still needed
*separately*, because the counter stays process-wide and the exact-delta premise
stays wrong even once it is atomic. Two independent changes, both required.

**Generalisable lesson.** A `// SAFETY:` comment that asserts a *whole-program*
property ("single-threaded access") rather than a local invariant is a claim
about code that does not exist yet — here, about every future caller and about
the test harness's threading model, neither of which the author controls. Two
different tests, filed by two different lanes on the same day, each blamed the
test rather than the counter, because the SAFETY comment read as settled. When
triaging a flake, check whether the thing being measured is itself sound before
concluding the assertion is too strong.

**✅ FIXED 2026-08-15 (lane B).** Exactly the recommended fix: `TICK_COUNTER` is
an `AtomicI64` and `times()` returns `fetch_add(1, Relaxed) + 1`, so the
`unsafe` block and its false `// SAFETY: single-threaded access` comment are
both gone. Covered by `test_times_hands_out_distinct_ticks_across_threads`
(8 threads x 200 calls released together through a bounded spin barrier,
asserting every returned tick is distinct); the barrier is load-bearing,
because without it the first thread finishes before the last is spawned and a
racy implementation would pass vacuously.

**Process note, worth more than the bug.** Lane B found this race independently,
by reading the counter while fixing lane C's flake, and wrote it up as "the half
nobody had noticed" — because lane A's request for it was sitting on
`origin/main`, unmerged and therefore invisible in lane B's worktree. Two lanes
diagnosed the same `static mut` on the same day and neither could see the
other's writeup. This is precisely the failure mode `CLAUDE.md` warns about:
`requests/` and the shared docs are *files on a branch*, not a mailbox. Merging
`origin/main` at the **start** of a task, not just before pushing, is what
prevents it.
