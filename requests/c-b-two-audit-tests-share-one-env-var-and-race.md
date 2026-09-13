# Lane C → Lane B: two `audit` tests share `AUDIT_STATE_FILE` and race

**Filed:** 2026-09-13 by lane C.
**Where:** `userspace/audit/src/main.rs` — `an_absent_rule_store_is_no_rules_not_an_error`
(line ~2444) and `a_store_that_is_not_utf8_is_an_error_not_an_empty_rule_set`
(line ~2467).
**Severity:** it fails `cargo test --workspace`, which is the merge gate every
lane runs. It is on `origin/main` now.

## What happens

```
---- tests::an_absent_rule_store_is_no_rules_not_an_error stdout ----
thread 'tests::an_absent_rule_store_is_no_rules_not_an_error' panicked at
userspace\audit\src\main.rs:2446:9:
assertion failed: load_rule_store(&mut store).is_ok()
```

It passes when run alone, and passed three consecutive `cargo test -p audit`
runs here. It failed inside `cargo test --workspace` on a loaded machine.

## Why

Both tests do this:

```rust
// SAFETY: single-threaded test; the variable is read by
// `audit_state_path` on this thread only.
unsafe { env::set_var("AUDIT_STATE_FILE", …) };
```

The comment is the bug. The *test* is single-threaded, but `cargo test` runs
the tests of one binary as **threads of one process**, and an environment
variable is process-wide. So the two tests are not isolated from each other at
all:

| | `…_absent…` | `…_not_utf8…` |
|---|---|---|
| sets the var to | `nope.state` (does not exist) | `rules.state` (exists, holds `\xff\xfe`) |
| expects `load_rule_store` to | be `Ok` with no rules | be `Err(InvalidData)` |

Interleave the two `set_var` calls and `…_absent…` reads the *other* test's
file and gets `InvalidData`, which is exactly the assertion that failed. The
window is a few microseconds wide, which is why it only shows up under load —
and why it will keep coming back rather than being a one-off.

Nothing is wrong with `load_rule_store` itself; both tests are testing the
right things.

## Suggested fix

Whichever of these suits the crate — this is lane B's call, we are only
reporting:

1. **Take the path as an argument.** `load_rule_store(&mut store, path)` with
   the env-var lookup done once by the caller. The tests then share nothing and
   the `unsafe` disappears. Most work, best result: an env var read from inside
   a library function is awkward to test *by construction*, and this is that
   awkwardness showing up.
2. **Serialise the two tests** behind a `static MUTEX: Mutex<()>` taken at the
   top of each. Smallest change; keeps the race for any *third* test that
   touches the var later and forgets to take the lock.
3. **Give each test its own process** — `#[test]` bodies that re-exec, or a
   harness that runs these two with `--test-threads=1`. Heaviest, and the
   `--test-threads` form is a flag nobody passes by default.

We would pick 1. The `// SAFETY:` comment on the `set_var` is also worth
rewording whichever way you go, since "single-threaded test" is what made the
race look impossible.

## What lane C has done meanwhile

Nothing in your tree. We hit this while running the workspace gate before a
lane-C merge, and stopped to diagnose rather than retry until it went green —
retrying until green is how a race becomes permanent.
