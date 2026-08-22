# c → b: `sudo_honours_a_delay_earned_at_another_prompt` races the wall clock

**Filed:** 2026-08-22 by lane C
**Your file:** `userspace/sudo/src/main.rs:3539` (the test), against
`userspace/authlib/src/lib.rs` (`retry_after`, `delay_for`, `with_clock`)

## What happened

A full `cargo test --workspace --target x86_64-pc-windows-gnu` from lane C
failed on your tree, not mine:

```
---- tests::sudo_honours_a_delay_earned_at_another_prompt stdout ----
thread '...' panicked at userspace\sudo\src\main.rs:3559:9:
a delay earned at another prompt must be honoured here
test result: FAILED. 241 passed; 1 failed
```

It is **flaky, not broken**: the same test passes 5/5 when run alone
(`cargo test -p sudo --target x86_64-pc-windows-gnu
sudo_honours_a_delay_earned_at_another_prompt`, ~0.1s each). It failed only
inside the full workspace run, where several hundred other test binaries are
competing for the machine. Nothing in the merge that preceded this run touched
`userspace/**` — it was `kernel/**` and shared docs only — and the previous
full workspace gate from this lane was green, so this is a latent race that
load exposes rather than a regression.

## Why it races

The faillock tally is stamped in **whole seconds**
(`Tally::last_failure_secs: u64`), and `retry_after` reports a delay only while
`now < last_failure_secs + delay_for(failures)`.

The test's setup loop stops at the *first* delay it earns:

```rust
while elsewhere.rate_limited("alice").is_none() {
    elsewhere.note_failure("alice");
}
```

With `FREE_ATTEMPTS = 3`, the first non-zero `delay_for` is `1 << 0` = **one
second**. So the test arranges the shortest delay the policy can produce and
then asserts a second `Authenticator` still sees it — after
`auth_fixture()` and two file-opening constructor calls have run in between.

At one-second resolution the surviving window is not "one second" but "whatever
is left of the current second", i.e. anywhere from ~0s to 1s depending on where
in the second `note_failure` landed. Unloaded, the gap is ~0.1s and almost
always lands inside the same second. Under a loaded workspace run, crossing the
boundary is easy — and when it does, `retry_after` correctly returns `None` and
the assertion correctly fails. **The production code is right; the test is
asking a time-dependent question.**

## Suggested fix

`Authenticator::with_clock(now: fn() -> u64)` is already public
(`userspace/authlib/src/lib.rs:528`), and the test is otherwise entirely
deterministic. Pinning the clock removes the race without weakening what the
test proves — that two authenticators over one faillock file share a tally,
which is the actual subject:

```rust
fn frozen() -> u64 { 1_000_000 }   // any fixed instant

let mut elsewhere = authlib::Authenticator::with_stores(missing, missing)
    .with_faillock(&faillock)
    .with_clock(frozen);
// ... and the same .with_clock(frozen) on the `sudo` side
```

Both halves then agree on `now`, the earned delay is honoured deterministically,
and the test no longer depends on how busy the machine is. Burning to a longer
delay instead (looping past several failures so `delay_for` reaches 8s or 16s)
would also work, but it papers over the dependency rather than removing it, and
it makes the test slower for no gain.

Two other things worth a glance while you are in there, both about the same
test rather than about the library:

- The scratch directory is a **fixed** path,
  `std::env::temp_dir().join("sudo-faillock-share-test")`, and the test opens
  with `remove_dir_all` on it. Nothing else in this binary uses that path today,
  so it is not the cause here — but two concurrent runs of the sudo test binary
  (two lanes building at once, which happens routinely) would delete each
  other's faillock file mid-test. A unique suffix would close that.
- The scratch directory is never cleaned up on success.

## Not fixed here

`userspace/**` is lane B's, so I have not touched it. This did not block me —
the failure is unrelated to my change (`gui/desktop` palette threading) and
everything in `gui/**` passed — but it will fail your workspace gate and mine
at random, so it is worth a deterministic clock.
