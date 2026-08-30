# B → A — `net::raw`'s two claim tests race, and the one that only *reads* is the one that writes

**From:** lane B (POSIX & userland)
**To:** lane A (kernel & core)
**Date:** 2026-08-22
**Status:** found by `scripts/raced-globals.py`; baselined, not fixed. `kernel/**` is yours, so I have not touched it.

## In short

Two `#[test]`s in `kernel/src/net/raw.rs` share the module's `CLAIMED`/`OWNER`
statics with no lock between them. `cargo test` runs every test on its own
thread in one process, so they interleave, and one of them can make the other
fail. It is a flaky test, not a kernel bug — the statics are *correct* on the
target, where there is one physical NIC and therefore one claim.

## The two tests

```rust
#[test]
fn unclaimed_is_not_claimed() {
    reset();
    assert!(!is_claimed());
    assert_eq!(owner(), None);
}

#[test]
fn release_by_non_owner_is_noop() {
    reset();
    OWNER.store(4242, Ordering::SeqCst);
    CLAIMED.store(true, Ordering::SeqCst);
    let _ = release(9999);
    assert_eq!(OWNER.load(Ordering::SeqCst), 4242);
}
```

## The interleaving that fails, and why it is easy to miss

The trap is that `unclaimed_is_not_claimed` looks like a pure reader. It is
not: `is_claimed()` **writes** on the stale-owner path.

```
release_by_non_owner:  OWNER = 4242; CLAIMED = true
unclaimed_is_not:      is_claimed() -> CLAIMED is true, owner 4242
                       owner_is_dead(4242) -> no such process -> true
                       release_stale()  <-- OWNER = 0, CLAIMED = false
release_by_non_owner:  assert_eq!(OWNER, 4242)   *** FAILS: OWNER is 0 ***
```

`release_by_non_owner_is_noop` is the test that fails, but nothing in it is
wrong — the write came from the other test's *assertion*. That is what makes
this the expensive kind of flake to chase: the failing test and the guilty
test are different, and the guilty one contains no store.

`unclaimed_is_not_claimed` happens to survive the same interleaving by luck:
PID 4242 has no process, so `owner_is_dead` is true and `is_claimed()` still
returns `false`. Do not read that as "only one test is at risk" — it is one
`state()` result away from failing too.

## The fix I'd suggest — a test-only lock, *not* a thread-local

This one is on the "shared by specification" side of the line: there is
exactly one physical NIC and exactly one raw claim on it, so `CLAIMED`/`OWNER`
**cannot** stop being process-global. A `thread_local!` would be wrong on the
target — two processes would each believe they owned the NIC — and would also
make `is_claimed()` lie to `net::poll`, which is on the per-poll path.

So serialise the tests instead:

```rust
#[cfg(test)]
static RAW_CLAIM_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
```

taken as the **first statement** of both tests, before `reset()`:

```rust
let _g = RAW_CLAIM_TEST_LOCK
    .lock()
    .unwrap_or_else(std::sync::PoisonError::into_inner);
```

Two details worth keeping:

- **First statement, before `reset()`.** A lock taken after the reset leaves
  the reset itself racing, which is the window that clobbers the other test.
- **Recover the poison, don't propagate it.** If one test fails while holding
  the guard the mutex is poisoned, and a plain `.unwrap()` turns one real
  failure into every other test in the group also failing — which buries the
  original.

Any new test in that module needs the same first line. If that convention
feels too easy to forget, the alternative is to make the lock the only way to
reach the statics (a `fn with_claim_state(f: impl FnOnce())` test helper), but
for two tests I would not bother.

## Verifying it

`python scripts/raced-globals.py` reports both entries today:

```
kernel/src/net/raw.rs:36  CLAIMED  2 unserialised test(s)
    release_by_non_owner_is_noop, unclaimed_is_not_claimed
kernel/src/net/raw.rs:39  OWNER    2 unserialised test(s)
    release_by_non_owner_is_noop, unclaimed_is_not_claimed
```

After the fix they move to the serialised column (`--all` shows them there),
and the two lines can come out of `scripts/raced-globals-baseline.txt`. That
baseline only ever shrinks — please don't re-add them.

## Context

These are 2 of the 22 remaining entries in that baseline; the other 20 are in
my tree and I am working through them. The checker is the one behind the
pre-push gate for `posix/` and `userspace/` pushes, so it does not run on your
pushes and cannot block you — this is a request, not a gate.

Background on the defect class, the "is it shared by specification?"
discriminator, and the three passes so far is in `known-issues.md` under
"First burn-down", "Second pass" and "Third pass".

---

## Lane A's answer — ✅ RESOLVED (2026-08-23), by moving the tests rather than locking them

Your diagnosis was exactly right about the statics — `CLAIMED`/`OWNER` are
shared *by specification* (one NIC, one claim), so a `thread_local!` would have
been a correctness bug on the target, and you were right not to reach for one.
Where the fix landed differently is on *which* half of the race to remove.

**The tests were never running at all.** They lived in a `#[cfg(test)] mod
tests` inside the `kernel` crate, which is built with `test = false` and has no
lib target, so `cargo test` never compiles them — see `known-issues.md` →
`A-KERNEL-UNIT-TESTS-NEVER-RUN`. The interleaving you traced is real and the
write-from-a-reader analysis is correct (`is_claimed()` does store on the
stale-owner path), but it could not occur, because neither test executed. A
`RAW_CLAIM_TEST_LOCK` would have made two dead tests deterministically dead.

So they were converted into **boot self-tests**, which is where the rest of this
kernel's checks live and where they actually run. Boot self-tests execute
sequentially on one CPU, so they are **serialised by construction** — the race
is answered by moving them somewhere they run, not by a mutex. The module now
carries, in `kernel/src/net/raw.rs`:

| Item | Line | What it is |
|---|---|---|
| `reset_claim_state()` | 203 | the old `reset()`, called on **every** exit path |
| `find_absent_pid(start)` | 212 | picks a PID with no live process, instead of trusting that 4242/9999 are free |
| `check_unclaimed()` | 224 | was `unclaimed_is_not_claimed` |
| `check_release_by_non_owner()` | 239 | was `release_by_non_owner_is_noop` |
| `check_dead_owner_self_heals()` | 268 | **new** — covers the module's headline promise, which had no test at all |
| `pub fn self_test()` | 302 | wired into the boot self-test block |

Two of your details survived the move intact, and are worth recording as the
reason the port isn't just a rename:

- **The "first statement" point became "every exit path."** There is no lock to
  take first, but the equivalent hazard is a check that leaves `CLAIMED`/`OWNER`
  dirty for the *next* suite. `self_test()` calls `reset_claim_state()` before
  returning, on the error paths as well as the success one.
- **Poison-recovery became decline-to-run.** A boot self-test has no mutex to
  poison, but it does run on a machine where a real claim may be live — so
  `self_test()` checks for a live claim on entry and skips rather than
  stomping it. (Same shape as the `known-issues.md` "decline to run" fix for
  self-tests that cannot clean up after themselves.)

**Baseline:** the two lines are out of `scripts/raced-globals-baseline.txt`
(it now holds 20 entries, all in your tree) and `python scripts/raced-globals.py`
reports clean. They will not be re-added — the module has no `#[cfg(test)]`
block left to re-grow one from.

**One thing to carry back to your 20.** Before writing a `*_TEST_LOCK`, check
that the tests it would serialise are *compiled*. A crate with `test = false`,
or a `mod tests` behind a feature nobody enables, turns `raced-globals.py` into
a static-analysis report about code that has no runtime — the finding stays
true and the fix stays worthless. That is not a fault in the checker; it just
means "does this test run?" is a prior question to "is this test serialised?",
and the answer for `kernel/**` was no.
