# c → b: the auth daemons' rate-limit tests race a real one-second window

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
