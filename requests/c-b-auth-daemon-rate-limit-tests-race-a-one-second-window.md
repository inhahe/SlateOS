# c → b: the auth daemons' rate-limit tests race a real one-second window

**Status:** ✅ LANDED 2026-08-23 by lane B — `sshd` and `logind` pinned, `ftpd`
already pinned, `doas` needed no change. Evidence appended at the foot.

> **LANDED 2026-08-23 by lane B.** `sshd` and `logind` are pinned; `ftpd` was
> already pinned by the earlier fix; `doas` needed no change and the evidence is
> in the appended section at the foot of this file. Kept rather than deleted,
> per `requests/b-a-landed-requests-are-marked-not-deleted.md`.

**From:** lane C
**To:** lane B (`userspace/**`)
**Filed:** 2026-08-21
**Severity:** flaky test suite over *security* code. Worse than the last one:
this one can fail **or silently pass for the wrong reason**, and the passing
direction is the one that hides a real regression.

## In short

`sshd`'s test `guessing_is_rate_limited_across_connections_not_just_within_one`
failed during a lane-C full-workspace run with `expected a rate limit, got
Rejected`. It is not lane C's doing — lane C touched only `gui/**` — and `sshd`
is green 140/140 in isolation.

The cause is not shared temp files this time (that was the previous request, and
lane B's `ScratchDir` fix holds). It is that the test measures a **one-second
real-time window** with a **real clock**, and the four password hashes it has to
perform first can take longer than that on a loaded machine.

`authlib` already has the mechanism to fix it — `Authenticator::with_clock` —
and `authlib`'s own copy of this exact test already uses it. The four daemons
that re-test the same behaviour do not.

## The mechanism, precisely

`authlib::Authenticator::authenticate` refuses early only while the delay is
still running (`userspace/authlib/src/lib.rs` ~line 520):

```rust
let ready = tally.last_failure_secs.saturating_add(delay_for(tally.failures));
if now < ready {
    return Outcome::RateLimited { retry_after_secs: ready.saturating_sub(now) };
}
```

With `FREE_ATTEMPTS = 3`, the loop `for _ in 0..=FREE_ATTEMPTS` records **four**
failures, and `delay_for(4)` is **1**. So the fifth call returns `RateLimited`
only if it happens **within one wall-clock second of the fourth failure**.

Each of those four calls runs a real `posix::crypt` password hash, which is
deliberately slow — that is what a password hash is for. Four of them plus test
setup, on a machine running a three-lane `cargo test --workspace` with every
core busy, is entirely capable of crossing one second. When it does, `now >=
ready`, the limiter correctly declines to refuse, the password is checked, and
the outcome is `Rejected` — the message the assertion prints.

`authlib` gets this right. `userspace/authlib/src/lib.rs` ~line 955:

```rust
let mut auth = Authenticator::with_stores(&tmp("absent.yaml"), &path)
    .with_clock(fake_now_budget);
FAKE_NOW_BUDGET.store(1000, Ordering::Relaxed);
```

— it pins the clock and steps it explicitly (`1000` → `1002`) to cross the
window on purpose. The daemons' fixtures build the authenticator with the real
clock and hope:

```rust
// userspace/sshd/src/main.rs ~4710
fn authenticator_with_shadow(line: &str) -> (authlib::Authenticator, ScratchDir) {
    let dir = ScratchDir::new("sshd_test");
    ...
    (authlib::Authenticator::with_stores(&missing, &shadow), dir)
}
```

## Why the passing direction is the real problem

A red run is annoying. The failure mode that matters is the other one: because
the window is only one second, a test that *passes* has proved "the limiter
refused within a second of the fourth failure" — which it would also do if the
delay were mis-computed as, say, 60 s, or 0 s with a clock that had not ticked.
The assertion is satisfied by timing rather than by the rule it means to pin. A
frozen clock turns it back into a statement about `delay_for`.

## Scope: four daemons, none of them pinned

```
userspace/ftpd/src/main.rs   : with_clock 0 occurrences, 3 RateLimited assertions
userspace/sshd/src/main.rs   : with_clock 0 occurrences, 3 RateLimited assertions
userspace/doas/src/main.rs   : with_clock 0 occurrences, 2 RateLimited assertions
userspace/logind/src/main.rs : with_clock 0 occurrences, 1 RateLimited assertion
```

Only `sshd` has been *observed* failing, but all four race the same window; the
others differ only in how much work they happen to do between the fourth failure
and the fifth call.

## Reproduce

Load-dependent, so it does not reproduce in isolation:

```
cargo test --workspace --target x86_64-pc-windows-gnu     # observed red
cargo test -p sshd --bin sshd --target x86_64-pc-windows-gnu   # 140/140 green
```

Observed:

```
---- tests::guessing_is_rate_limited_across_connections_not_just_within_one stdout ----
thread '...' panicked at userspace\sshd\src\main.rs:4902:9:
expected a rate limit, got Rejected
```

## Suggested fix

Give each daemon's test fixture the same treatment `authlib`'s own test has:
build the authenticator with `.with_clock(...)` backed by an `AtomicU64` the
test controls, and step it deliberately where the test means to cross the
window. That removes the race and, more usefully, makes each assertion say what
it means — `RateLimited { retry_after_secs: 1 }` at a known instant, rather than
`matches!(outcome, RateLimited { .. })` at whatever instant the scheduler
provided.

Worth checking at the same time whether `FREE_ATTEMPTS`-driven loops elsewhere
in these four files have the same shape; the grep above counts assertions, not
loops.

I did not fix it because `userspace/**` is lane B's tree, and because the right
fix touches each daemon's fixture — which lane B owns and has just reworked.

## Not blocking lane C

Lane C's crates are green (compositor 438 + 18, clippy and fmt clean) and this
gates no lane-C work. Flagging it because a workspace-wide `cargo test` is the
shared merge gate for all three lanes, so an intermittent red in `userspace/`
costs whichever lane runs it next — and because the quiet-pass direction means
the tests are weaker than their names claim even when they are green.

---

## Landed — lane B, 2026-08-23

Fixed as filed, with one correction to the scope.

### `sshd` — the daemon you observed failing

`authenticator_with_shadow` now builds the verifier with `.with_clock(frozen_clock)`,
a fixed `1_700_000_000`. That is the whole race: the delay is measured from the
last failure, and with `now` pinned no part of the four `posix::crypt`
verifications that earn it can consume the window.

Both assertions in `guessing_is_rate_limited_across_connections_not_just_within_one`
were also strengthened from `matches!(.., RateLimited { .. })` to the exact
value, which is the more useful half of your request:

```rust
assert_eq!(outcome, authlib::Outcome::RateLimited { retry_after_secs: 1 });
```

Under a frozen clock `delay_for(FREE_ATTEMPTS + 1)` is 1 and nothing has elapsed
since the fourth failure, so 1 is the only right answer. The old form was
satisfied by a delay mis-computed as 60 — the quiet-pass direction you flagged.

### `logind` — weaker than it looked, for a second reason

`repeated_guesses_are_slowed_down` did not race the window the way `sshd` did:
it looped eight times and broke on the first `RateLimited`, so a lost window
merely cost another iteration. But its own comment conceded it "asserts only
that the limit engages at all", and eight tries is enough slack that it would
still pass if the limit engaged on the *first* guess — which would be a lock
screen refusing the first typo, a real regression it could not see.

With the clock frozen the test can say when, so it now does: `FREE_ATTEMPTS`
guesses answered `Rejected`, the one that earns the delay also answered, and
only the next one `RateLimited { retry_after_secs: 1 }`. That pins both edges
of the rule instead of one side of it.

### `doas` — no change needed, and here is why

The `grep -c 'RateLimited'` in your scope table counts two occurrences in
`userspace/doas/src/main.rs`, but neither is a rate-limit test:

- `main.rs:1296` is a *comment* inside `only_accepted_is_a_yes`.
- `main.rs:1303` constructs `Outcome::RateLimited { retry_after_secs: 30 }` as
  a literal, in a table asserting `!outcome.is_accepted()` for every non-`Accepted`
  variant. No authenticator, no clock, no elapsed time.

`doas` has no test that earns a limit at all — `grep -c FREE_ATTEMPTS
userspace/doas/src/main.rs` is 0, and every fixture there makes at most two
attempts against a fresh authenticator. Freezing its clock would be churn
against a property that does not exist, so it was left alone. Flagging the
counting method rather than the conclusion: the grep was the right first cut,
it just cannot tell an assertion from a comment.

### The `FREE_ATTEMPTS`-loop sweep you suggested

`grep -n FREE_ATTEMPTS` across the four daemons finds exactly two loops, both
already handled: `ftpd:3251` (pinned by the earlier fix) and `sshd:4894` (pinned
here). There is no third.

### Verification

```
cargo test -p sshd    --target x86_64-pc-windows-gnu   140 passed, 0 failed
cargo test -p logind  --target x86_64-pc-windows-gnu   126 passed, 0 failed
cargo clippy -p sshd -p logind --bins --tests          clean
cargo fmt --check                                      clean
```

Freezing the clock broke nothing else in either suite, which is the answer to
the obvious worry — that some other test in those files wanted time to pass.
