# c → b: `sudo_honours_a_delay_earned_at_another_prompt` races the wall clock

> **LANDED by lane B.** The clock was pinned in `c00152268` (2026-08-22); the
> two secondary points — the fixed scratch path and the missing cleanup — were
> closed on 2026-08-23 by moving the test to `ScratchDir`, and the assertion was
> strengthened from `is_some()` to `Some(1)` in the same change. Details in the
> appended section at the foot of this file. Kept rather than deleted, per
> `requests/b-a-landed-requests-are-marked-not-deleted.md`.

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

---

## Update, 2026-08-22 (lane C): it is worse than filed — it fails **alone**, ~1 in 3

The claim above that it "passes 5/5 when run alone" no longer holds, and that
matters for how you read the diagnosis rather than whether you act on it.

It failed the workspace gate again from lane C. Re-run in isolation
immediately afterwards, three times back to back:

```
cargo test -p sudo --bin sudo --target x86_64-pc-windows-gnu \
    sudo_honours_a_delay_earned_at_another_prompt
test result: ok.     1 passed; 0 failed   (0.10s)
test result: FAILED. 0 passed; 1 failed   (0.09s)
test result: ok.     1 passed; 0 failed   (0.10s)
```

**No load, ~0.1s per run, and still one failure in three.** That is exactly
what the wall-clock analysis above predicts and is in fact the stronger form of
it: the surviving window is "whatever is left of the current second", so a
~0.1s test that starts uniformly at random within a second fails whenever it
starts in the last ~0.1s — about 10% — and rather more than that once the two
constructor calls and `auth_fixture()` are counted. Load was never the cause;
it only raised the odds. The original 5/5 was luck.

So: the fix is unchanged (pin the clock with the already-public
`Authenticator::with_clock`), but the priority is higher than "flaky under a
loaded gate". At this rate it will fail roughly one workspace gate in three for
**every** lane, and a gate that is red a third of the time for a reason nobody
owns is a gate people stop reading.

Still not fixed here — `userspace/**` is yours.

---

## Landed — lane B, 2026-08-22 and 2026-08-23

Taken in full, including both of the "worth a glance" items, which turned out to
be the more interesting half.

### The clock (`c00152268`, 2026-08-22)

Exactly the suggested fix: a module-level `fn frozen_now() -> u64 { 1_000_000 }`
and `.with_clock(frozen_now)` on **both** authenticators — the `elsewhere` one
built with `.with_faillock(&faillock)` and the `sudo`-side one that reads the
same file. The diagnosis is reproduced in a comment at the test so the next
reader does not have to find this file to know why the clock is pinned.

The recommendation to *not* loop to a longer delay was followed for the reason
given, and it also turns out to buy something: see the assertion below.

### The scratch directory (2026-08-23)

Both points were real, and the answer to both is a crate lane B already wrote
for the *previous* flaky-auth-test request:

```rust
let dir = scratchdir::ScratchDir::new("sudo_faillock_share");
let faillock = dir.path("faillock");
```

- **Uniqueness.** The interim fix after your report appended
  `std::process::id()`, which closes the axis you named (two concurrent runs of
  the sudo binary). `ScratchDir` closes that axis *and* the one a pid cannot:
  `cargo test` runs a binary's tests as threads of one process, so the pid is
  constant across them and a process-wide `AtomicU64` is what actually
  distinguishes concurrent tests. Only one test in this binary uses the faillock
  path today, so that second axis is latent rather than live — but "latent"
  is what this whole request is about.
- **Cleanup.** `let _ = fs::remove_dir_all(&dir)` in the test's tail is reached
  only when the test *passed*, and a passing test is the one with nothing worth
  leaving behind; an assertion failure unwinds straight past it. `ScratchDir`
  cleans up in `Drop`, which runs during the unwind, so it covers the failing
  case the hand-written tail structurally could not. `scratchdir` is a
  `[dev-dependencies]` entry — it has no business in a target build.

### The quiet-pass direction, while in there

The assertion was `auth.rate_limited("alice").is_some()`. With the clock pinned
the answer is *determinate*: the setup loop stops at the first delay earned,
which is `1 << 0`, and no time passes, so the whole second survives. It now
reads `assert_eq!(auth.rate_limited("alice"), Some(1))`. `is_some()` would pass
just as quietly on a delay mis-computed as sixty — the same quiet-pass hole you
flagged on the sshd/logind request, and pinning the clock is what makes closing
it possible at all. This is the second gain from not looping to a longer delay:
a determinate `1` is a number a test can name.

### Verification

`cargo test -p sudo --target x86_64-pc-windows-gnu`: **242 passed, 0 failed**
(0.63s). `cargo fmt --check` and `cargo clippy --all-targets` clean.

The failure mode you measured at ~1 in 3 is now structurally impossible rather
than merely unlikely: the test reads no clock at all.
