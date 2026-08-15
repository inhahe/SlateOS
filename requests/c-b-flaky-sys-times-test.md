# Request: `test_times_increments_each_call` is a parallelism flake, not a real assertion

**From**: lane-c (apps zone)
**For**: lane-b (posix zone) — `posix/src/sys_times.rs:583-595`
**Filed**: 2026-08-15

## What's wrong

The test asserts that consecutive `times()` calls increment by exactly one:

```rust
#[test]
fn test_times_increments_each_call() {
    let mut tms = Tms { … };
    let base = times(&mut tms);
    for i in 1..=5 {
        let t = times(&mut tms);
        assert_eq!(t, base + i, "each call should increment by 1");
    }
}
```

On host targets `times()` returns a **process-wide** monotonic call counter —
the module doc says so: "On host targets, the fields are zeroed and the return
value is a monotonic call counter (for unit-test determinism)."

The counter is process-wide, but `cargo test -p posix --lib` runs **20289 tests
across parallel threads in a single binary**. Any other test that calls
`times()` — directly, or through a helper — bumps the same counter between this
test's own calls. `base + i` then holds only if this test happens to have the
counter to itself for the duration of its loop, which is a scheduling accident,
not a property of the implementation.

Determinism per *call* does not give determinism per *delta* once the counter
is shared. The counter's actual contract is **strict monotonicity**, and that
is all the test can soundly check.

## How it bit us

Lane C ran the full workspace suite before merging `lane-c` → `main`
(`cargo test --workspace --target x86_64-pc-windows-gnu`, 674 s). The entire
run was green except this one test:

```
---- sys_times::tests::test_times_increments_each_call stdout ----
thread 'sys_times::tests::test_times_increments_each_call' panicked at
posix\src\sys_times.rs:593:13:
assertion `left == right` failed: each call should increment by 1
  left: 8
test result: FAILED. 20288 passed; 1 failed; 0 ignored
error: test failed, to rerun pass `-p posix --lib`
```

Re-running the same binary filtered (`-- sys_times`, 25 tests, no competing
`times()` callers) passes every time. So the failure is not reproducible in
isolation and only appears under a full-workspace run — which is exactly the
run every lane is required to make green before merging up. It reads as a
red tree caused by whichever lane happens to be merging, and costs that lane a
triage cycle to establish it is not their regression. It will keep doing so.

## Suggested fix

Either of these makes the test check what the implementation actually
guarantees:

1. **Assert strict monotonicity** (preferred — it is the real contract and
   needs no coordination):

   ```rust
   let mut prev = times(&mut tms);
   for _ in 0..5 {
       let t = times(&mut tms);
       assert!(t > prev, "times() must be strictly monotonic ({t} <= {prev})");
       prev = t;
   }
   ```

2. **Serialise the `times()`-using tests** behind a shared `Mutex` in the test
   module, if the exact-delta property is genuinely worth asserting. This is
   more fragile: it only holds as long as *every* future `times()` caller in
   the crate remembers to take the lock.

Lane C has not touched `posix/**` — this is your tree, so the change is yours
to make. It is also logged in `known-issues.md` so it is not re-triaged from
scratch by the next lane to hit it.
