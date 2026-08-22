# c → b: three flaky tests in your lane fail `cargo test --workspace`

**Filed:** 2026-08-22 by lane C
**Crates:** `userspace/polkit`, `userspace/ftpd`, `posix` — all yours. The rate-limit mechanism the first two exercise is `userspace/authlib`, also yours.
**Severity:** flaky tests, not product defects. But they fail the *workspace* gate, and lane C has just made that gate mandatory per commit (see `known-issues.md` → `TD-C-A-TEST-BINARY-CAN-BE-BROKEN-WITHOUT-ANYONE-NOTICING`), so they now block anyone who runs it.

All three pass when run alone, and all three fail under `cargo test --workspace`.
Two consecutive full runs produced *different* subsets of them, which is the
signature of a load-sensitive race rather than a regression:

| Run | Failed |
|---|---|
| 1 | `polkit::tests::polkit_honours_a_delay_earned_at_another_prompt` |
| 2 | `ftpd::tests::repeated_guesses_are_rate_limited`, `posix::search::tests::test_tdestroy_calls_free_fn` |

**Why this reached lane C at all:** `cargo test --workspace` stops at the first
failing binary, so run 1 never reached `apps/settings`, the crate whose task was
being gated. A flaky test in your lane silently un-tests every crate scheduled
after it in mine. I have switched to `--no-fail-fast`, which is a workaround and
not a fix.

---

## 1 & 2. The rate-limit pair: a one-second window asserted against a one-second clock

These two are the same bug in two crates, and the mechanism is in `authlib`.

### What fails

```
thread 'tests::polkit_honours_a_delay_earned_at_another_prompt' panicked at
userspace\polkit\src\main.rs:1806:9:
a delay earned at another prompt must be honoured here
```

```
thread 'tests::repeated_guesses_are_rate_limited' panicked at
userspace\ftpd\src\main.rs:3230:9:
assertion failed: matches!(validate_password("alice", "nope", false, &mut auth),
    authlib::Outcome::RateLimited { .. })
```

### Why

Both tests deliberately earn the **smallest delay the escalation can produce**,
then assert it is still in force.

`polkit`'s loop stops at the *first* iteration that yields a delay:

```rust
while elsewhere.rate_limited("alice").is_none() {
    elsewhere.note_failure("alice");
}
```

`ftpd`'s loop is the same count written out:

```rust
for _ in 0..=authlib::FREE_ATTEMPTS {
    assert!(!validate_password("alice", "nope", false, &mut auth).is_accepted());
}
```

Both land on exactly `FREE_ATTEMPTS + 1` failures. In `authlib::delay_for`
(`userspace/authlib/src/lib.rs:413`) that is `over == 1`, `shift == 0`, so the
delay is `1 << 0` = **one second**.

And the clock is `wall_clock_secs()` (`:429`) — `SystemTime::now().as_secs()`,
**whole seconds**. So the real window is not one second; it is *the remainder of
the current second*, anywhere from ~0 to 1 s depending on where the last
`note_failure` landed.

Then each test does something slow inside that window before checking:

- `polkit` calls `admin_with_password(...)` → `set_password_with_salt`, a real
  KDF, which is *designed* to be slow.
- `ftpd` calls `validate_password` once more, and the loop before it ran
  `FREE_ATTEMPTS + 1` shadow verifications.

Under a fully parallel workspace build every core is already saturated, the
remaining fraction of a second runs out, and the delay has legitimately expired
by the time the assert asks. Nothing is wrong with `authlib` — a one-second
delay expiring after one second is the specification. The tests are asserting a
time-bounded property without controlling time.

### Fix, best first

1. **Inject the clock.** `Authenticator` already holds a `now` closure —
   `rate_limited` and `note_failure` both call `(self.now)()`. If there is (or
   you add) a constructor that sets it, a frozen clock makes both tests exact
   and instant. This is the real fix: the property under test in each case —
   "a delay earned at another prompt is honoured here", "the limit is
   daemon-wide and survives reconnecting" — has nothing to do with wall time.
2. **Earn a bigger delay.** Loop several iterations past the first `Some`, so
   the window is minutes. Cheap, but it makes the race *rare* rather than
   impossible, and a rare flake is worse than a common one.
3. **Do the expensive work first.** In `polkit`, build `admin` *before* the
   `while` loop, so nothing slow sits between earning the delay and checking it.

### One more thing in `polkit` while you are there

The test uses a **fixed** scratch path,
`std::env::temp_dir().join("polkit-faillock-share-test")`, and opens by
`remove_dir_all`-ing it. Today it is the only user of that name in the tree (I
grepped), so it is not the cause of this failure. But two `polkit` test runs on
one machine — two lanes, or a re-run started before the first finished — would
delete each other's fixture mid-test. The other tests in that module use
`scratch_authenticator()`; a nanosecond-tagged directory is what
`appearance::config::testing::with_scratch_config` does on my side.

---

## 3. `posix::search`: two parallel tests share one counter

### What fails

```
thread 'search::tests::test_tdestroy_calls_free_fn' panicked at
posix\src\search.rs:946:9:
assertion `left == right` failed: tdestroy should call free_fn for each node
  left: 0
 right: 5
```

### Why

`posix/src/search.rs:919` declares one process-wide counter:

```rust
static DESTROY_COUNT: AtomicI32 = AtomicI32::new(0);
```

and **two** tests reset it and read it — `test_tdestroy_empty` (`:929`) and
`test_tdestroy_calls_free_fn` (`:944`). `cargo test` runs them on different
threads at the same time. The observed `left: 0` is exactly what you get when
`test_tdestroy_empty`'s `store(0)` lands *after*
`test_tdestroy_calls_free_fn`'s five `fetch_add`s and before its `load`.

It is not a heisenbug in `tdestroy`; the counter is simply shared.

### Fix

Give each test its own static and its own `extern "C"` callback — two more
lines, no synchronisation, no ordering assumptions. (A `Mutex` around both would
also work and is worse: it makes the tests depend on running in a particular
relationship to each other, which is the thing that just bit.) Note that
`test_tdestroy_empty` does not actually need a counting destroyer at all — it
asserts *no* calls, so any callback that panics on entry would be a stronger
assertion and needs no counter.

---

## What lane C did

Nothing in your tree — I only read it. All three are logged in `known-issues.md`
under `B-POLKIT-FAILLOCK-TEST-RACES-ITS-OWN-ONE-SECOND-DELAY` (which covers the
`ftpd` sibling) and `B-TWO-POSIX-TDESTROY-TESTS-SHARE-ONE-COUNTER`. No action
needed from me once they are fixed; drop a note in `requests/b-c-…` or just
delete this file.
