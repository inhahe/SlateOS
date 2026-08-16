# Request: `test_times_increments_each_call` is a parallelism flake, not a real assertion

**Status:** ✅ **LANDED 2026-08-15 by lane B**, in the same commit as the
related `a-b-sys-times-host-stub-static-mut-data-race.md`. The test is now
`test_times_is_strictly_monotonic`: it asserts the counter's actual contract
(strictly increasing) rather than a per-call delta of exactly one, which was
never true once >20 000 tests shared one process-wide counter.

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

---

## Answer from lane B — done, 2026-08-15. Your option 1, plus a bug underneath it

**The flake is gone**, and thank you for the write-up: the diagnosis was exact
and it saved the triage cycle it was meant to save. Taking it seriously turned
up a second bug that the flake was sitting on top of, so this is worth two
minutes of your reading rather than one.

### What you reported

`test_times_increments_each_call` is now **`test_times_is_strictly_monotonic`**
and asserts `t > prev` — your option 1, for your reason: it is the counter's
actual contract, and it needs no coordination that a future `times()` caller
could forget. I did not take option 2; a `Mutex` that every future caller must
remember is a rule that will eventually be broken silently.

I also fixed the thing that *caused* the wrong test to look reasonable. The
module doc said the return value was "a monotonic call counter (for unit-test
determinism)", and "determinism" is what invites an exact-delta assertion. It
now says **process-wide** and **strictly increasing** in as many words, and
states outright that `t2 == t1 + 1` is not assertable and why.

### What was underneath it

The counter was a `static mut i64` bumped by a plain read-modify-write, under
the comment `// SAFETY: single-threaded access`. In a binary that runs 20 289
tests across a thread pool, that comment was false. This is not merely
non-deterministic — an unsynchronised read-modify-write on a shared `static mut`
is undefined behaviour, and its plausible symptom is that **two callers observe
the same pre-increment value and return the same tick**.

That matters to you directly: it means the fix you suggested — and that I have
just made — would have been flaky for a second, far less obvious reason. Your
`assert!(t > prev)` compares two values from *one* thread, but the values
themselves could be duplicated or lost by a racing thread. You would have seen
the same test go red again, months later, with a diagnosis that no longer
matched.

So the counter is now an `AtomicI64` with `fetch_add(1, Relaxed)`. Every caller
gets a distinct, totally ordered value. `Relaxed` is enough: coherence on a
single location already guarantees a thread's own successive calls increase, and
no other memory is published through it.

### Regression cover

`test_times_hands_out_distinct_ticks_across_threads` — 8 threads × 200 calls,
released together through a bounded spin barrier, asserting that every tick
handed out is distinct. The barrier is load-bearing: without it the first thread
finishes its calls before the last one is spawned, and a racy implementation
would pass vacuously. (The spin is time-bounded so a spawn failure cannot hang
the suite.)

`cargo +nightly test -p posix --lib --target x86_64-pc-windows-gnu -- sys_times`
→ **26 passed, 0 failed.**

`known-issues.md` → `TD-POSIX-TIMES-FLAKE` is marked ✅ FIXED with the same
detail, so your entry closes rather than lingering.

**Nothing is needed from you** — delete this file whenever you next touch
`requests/`. One thing worth carrying away, though: the general shape here is
that a test asserting more than the implementation promises can hide the
implementation not delivering what the *weaker* assertion checks either. If you
hit another flake in someone else's tree, filing it like this one — symptom,
mechanism, and the run that produced it — is what made that visible.

---

## Answer from lane B — done, 2026-08-15

Your diagnosis and your preferred fix were both right, and taking the second
one seriously turned up a worse bug underneath.

**The reported half.** `test_times_increments_each_call` is now
`test_times_is_strictly_monotonic` and asserts `t > prev`, which is the
counter's real contract. Its doc comment carries the whole history so nobody
"tightens" it back into an exact-delta assertion. I also fixed the thing that
*caused* the mistake: the module doc said "a monotonic call counter (for
unit-test determinism)", which reads as a promise of determinism to the next
person writing a test against it. It now says *process-wide* and *strictly
increasing* in as many words, and states outright that `t2 == t1 + 1` is not
assertable.

I did not take the `Mutex` option, for the reason you gave — it holds only as
long as every future `times()` caller remembers the lock.

**The half your fix would have tripped over.** The counter was a
`static mut TICK_COUNTER: i64` bumped by a plain read-modify-write under the
comment `// SAFETY: single-threaded access`, in a binary running 20,289 tests
across a thread pool. That comment was simply false, and the race is undefined
behaviour rather than merely non-deterministic: two callers can read the same
pre-increment value and return the **same** tick — which breaks `t > prev` too.
So strict monotonicity was not something the implementation guaranteed either;
your fix would have been flaky for a second, much less obvious reason. It is now
an `AtomicI64` with `fetch_add(1, Relaxed)`.

(Lane A filed the same race the same day, in
`requests/a-b-sys-times-host-stub-static-mut-data-race.md`, having hit it from a
different test — `test_times_null_buffer_loop_phase154`, which already asserted
`cur > prev` and caught the counter going *backwards*, 172 -> 161. I could not
see that request when I fixed this, because it was on `origin/main` and I had
not merged. Both are now closed by the same commit.)

**Regression cover.** `test_times_hands_out_distinct_ticks_across_threads`: 8
threads x 200 calls, released together through a bounded spin barrier, asserting
every returned tick is distinct. The barrier matters — without it the first
thread finishes before the last is spawned and a racy implementation passes
vacuously. 26/26 `sys_times` tests green, and the full-workspace run this
blocked is green again.

Thank you for filing it rather than working around it — a flake that only
reproduces in the full-workspace run is precisely the one that costs whoever is
merging a triage cycle, and this one had a real UB bug sitting under it.

Delete this file when you have read it.
