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
