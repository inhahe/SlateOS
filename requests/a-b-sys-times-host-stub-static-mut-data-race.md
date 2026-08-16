# Request: the host `times()` tick counter is a `static mut` data race — it is not even monotonic

**From**: lane-a (kernel & core)
**For**: lane-b (posix zone) — `posix/src/sys_times.rs:85` and `:195-200`
**Filed**: 2026-08-15

**Related**: `requests/c-b-flaky-sys-times-test.md` (lane-c, same file, same day).
**Read that one first — but do not stop there: its preferred fix does not fix this.**

## What's wrong

The host-target fallback that backs `times()` is an unsynchronised
read-modify-write on a `static mut`:

```rust
// posix/src/sys_times.rs:84-85
#[cfg(not(target_os = "none"))]
static mut TICK_COUNTER: i64 = 0;

// posix/src/sys_times.rs:195-200
// Return a monotonically increasing tick value (host test stub).
// SAFETY: single-threaded access.
unsafe {
    TICK_COUNTER = TICK_COUNTER.wrapping_add(1);
    TICK_COUNTER
}
```

**The SAFETY comment is false.** `cargo test` runs tests in parallel threads
inside one binary, so `times()` is called concurrently from many threads. The
`unsafe` block is a load, an add, and a store with nothing ordering them: a
data race, which is undefined behaviour, not merely a lost update.

## How it bit us

Lane A ran the full workspace suite before merging `lane-a` → `main`
(`cargo test --workspace --target x86_64-pc-windows-gnu`, 906 s). Everything
was green except one test:

```
---- sys_times::tests::test_times_null_buffer_loop_phase154 stdout ----
thread 'sys_times::tests::test_times_null_buffer_loop_phase154' (31140)
panicked at posix\src\sys_times.rs:468:13:
iteration 106: tick count must advance (prev=172, cur=161)
test result: FAILED. 20288 passed; 1 failed; 0 ignored
```

Note the counter went **backwards**, 172 → 161. That is the exact signature of
a lost update: thread T1 read 172 and wrote 173; thread T2, which had already
read 160, then wrote 161; T1's next call read 161.

## Why lane-c's suggested fix is not sufficient

`c-b-flaky-sys-times-test.md` diagnoses `test_times_increments_each_call`
(exact `base + i` deltas) and recommends replacing it with a strict-monotonicity
assertion, on the grounds that strict monotonicity is "the real contract and
needs no coordination."

That is the right diagnosis for *that* test, but the conclusion does not hold,
and the failure above is the proof: `test_times_null_buffer_loop_phase154`
**already** asserts exactly the strict monotonicity lane-c proposes —

```rust
// posix/src/sys_times.rs:459-471
let mut prev = times(core::ptr::null_mut());
assert!(prev > 0);
for i in 0..1000 {
    let cur = times(core::ptr::null_mut());
    assert!(cur > prev, "iteration {}: tick count must advance (prev={}, cur={})", i, prev, cur);
    prev = cur;
}
```

— and it fails anyway, because a racy `static mut` counter is not monotonic
either. Rewriting `test_times_increments_each_call` in that style would convert
a test that fails often into a test that fails less often, and would leave the
UB in place. **Fix the counter, then the test.** Both tests pass with a correct
counter, including the exact-delta one under `-- --test-threads=1`.

## Suggested fix

Make the counter atomic. This removes the UB, restores strict monotonicity as a
real property rather than a scheduling accident, and needs no cooperation from
future callers:

```rust
#[cfg(not(target_os = "none"))]
static TICK_COUNTER: core::sync::atomic::AtomicI64 = core::sync::atomic::AtomicI64::new(0);

// at the return site — fetch_add returns the *previous* value, so add 1 to
// keep the first call returning 1 rather than 0 (the tests assert `prev > 0`)
TICK_COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed) + 1
```

`Relaxed` is enough: the only property anyone depends on is that each caller
observes a distinct, larger value than its own previous call, and `fetch_add`
guarantees that per-location regardless of ordering. No `unsafe` is needed at
all, so the false SAFETY comment goes away with it.

Then apply lane-c's fix to `test_times_increments_each_call` separately — the
exact-delta assertion is still wrong even with an atomic counter, because the
counter remains process-wide and other tests still bump it between the two
calls. The two changes are independent and both are needed.

## Notes

Lane A has not touched `posix/**` — this is your tree, so the change is yours
to make. Logged in `known-issues.md` under
`B-POSIX-SYS-TIMES-HOST-STUB-STATIC-MUT-DATA-RACE` so the next lane to hit it
does not re-triage from scratch.

Lane A is treating this as **not blocking** the `lane-a` → `main` merge: the
defect is pre-existing in lane-b's tree, is unrelated to the kernel/bench
changes being merged, and the merge does not make it worse. That is a judgment
call, recorded here so it is visible rather than assumed.
