## B-THE-STDIO-BUFFER-RACE-THAT-A-GREEN-TEST-RUN-COULD-NOT-SEE — 2026-08-21 — FIXED

**In short:** `cargo test -p posix` passed every time you ran it normally, and
failed 26 times out of 30 once the test *order* was randomised. Three `fclose`
tests were reading a `stdout` buffer that ten unrelated `wchar` tests write to.
Nothing is wrong with the library — this is purely an artefact of how the tests
run on the host — but the tests are now locked, and the guard cleans up on the
way out as well as on the way in.

**Symptom.** `stdio::tests::test_fclose_stdout_flushes_only` (and its `stdin` /
`stderr` siblings) assert `fclose` returns `0`. Under randomised ordering they
returned `EOF`. Under the default alphabetical order they never did — which is
why 20418 passing tests had said nothing about it.

**Root cause, and why it is a host-test artefact rather than a library bug.**
`posix/src/syscall.rs::syscall2` has a `#[cfg(not(target_os = "none"))]` arm
that returns `HOST_ENOSYS` unconditionally. So in a host `cargo test` build,
every `write()` to a `HandleKind::Console` fd *always fails*. `fclose` on a
stream with a non-empty buffer must flush, the flush cannot succeed, and
`fclose` correctly reports `EOF`. On the real target the write reaches the
console and the same code returns `0`. Do not "fix" `fclose`.

That makes the tests' real precondition "the stdout buffer is empty" — which
holds only if no other test has written to it. Ten tests in `wchar.rs`
(`fputwc` / `putwc` / `fputws` against `stdout`) leave bytes there.

**How it was found — the method matters more than the bug.** Three attempts at
finding this by static analysis (grepping for globals, matching writer function
names) all produced garbage: one classifier confidently reported that 26 files
wrote `SIM`, 25 wrote `AIO_LOCK`, 24 wrote `TIMEX_LOCK`, all from over-broad
name matching. What worked was empirical:

```
RUSTC_BOOTSTRAP=1 cargo test -p posix --target x86_64-pc-windows-gnu \
    -- -Z unstable-options --shuffle
```

varying `RUST_TEST_THREADS` over 8 / 32 / 64. 30 runs, **26 failing**. Then
`--shuffle-seed N` with `--exact` to pin the order: 3 of 3 fail when the `wchar`
tests run first, 3 of 3 pass when they run second. A deterministic split like
that is proof of order-dependence, not of a concurrency race — an important
distinction, because my first hypothesis (a pure race, inferred from eight
passing single-threaded seeds) was wrong, as was my second (that the poisoner
lived in `stdio.rs`).

**Fix.** `posix/src/stdio.rs` gains `STD_STREAM_TEST_LOCK` and
`lock_std_streams_for_test()`, returning a `StdStreamTestGuard` rather than a
bare `MutexGuard`. The guard purges the three standard streams' buffers *both*
on acquisition and **in `Drop`**. The drop half is the point: a test that writes
to `stdout` cannot leave residue for the next test even if its author never
thought about cleanup, so correctness does not depend on every future test
remembering. A plain `MutexGuard` would leave that obligation with the caller —
the same "looks sufficient, isn't" trap the deleted `PersonalityGuard` fell
into. The three `fclose` tests and the ten `wchar` tests take the lock.

**Verification.** The identical 30-run shuffled soak that produced 26 failures
now produces **0**, and the three previously-deterministic-failing seeds (2, 4,
6) pass.

**Bound on what remains.** The soak exercised all 20418 tests under randomised
order across 30 runs and surfaced exactly *one* order-dependent test beyond the
`personality` one — this. That is a useful negative result: the crate does not
have a swarm of these, so the shuffled soak is worth keeping as an occasional
check rather than a standing CI cost. It is not currently wired into any script;
re-run the invocation above by hand after adding tests that touch process
globals.
