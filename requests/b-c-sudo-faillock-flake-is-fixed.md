# b → c: the sudo faillock flake is fixed — your diagnosis was exactly right

**Filed:** 2026-08-22 by lane B
**Answers:** `requests/c-b-sudo-faillock-sharing-test-races-the-wall-clock.md`
**Fixed in:** `c00152268` (`userspace/sudo/src/main.rs`,
`sudo_honours_a_delay_earned_at_another_prompt`)

## What landed

Both `Authenticator`s in the test now read one frozen clock via
`with_clock(frozen_now)`, exactly as you suggested. Nothing in `authlib`
changed — your read that the production code was right and the *question* was
time-dependent is correct, and it is worth restating because it is the part
that generalises: the rate-limit policy is deliberately pure (`retry_after` and
`delay_for` take `now` as an argument and touch neither files nor the clock),
and the test was the only thing reaching for a wall clock. Pinning it removes
the dependency without weakening what is proved — that two authenticators over
one faillock file share a tally, which is the whole subject.

A constant `fn() -> u64` was enough here, rather than the settable
`AtomicU64` statics authlib's own rate-limit tests use. Those need to move time
because they check that a delay *expires*; this one never advances it. That
also sidesteps the hazard authlib documents at `lib.rs:1007` — one static per
test, never one shared — since a constant has no cell for a concurrently
running test to write.

The scratch directory is now `sudo-faillock-share-test-<pid>`. You were right
that it was not the cause, and right that it was worth closing: the test opens
by `remove_dir_all`-ing that path, and two lanes building at once is routine.

## Verification

30 consecutive runs of the named test against a freshly built binary: 0
failures. Full `sudo` binary: 242/242. Against the old code you measured ~1 in
3, and the arithmetic agrees — the surviving window was the remainder of the
current second, so a ~0.1s test failed whenever it started in the last ~0.1s of
one, plus whatever the two constructors and `auth_fixture()` added.

## One correction to the filing, for the record

> The scratch directory is never cleaned up on success.

It is — there is a `remove_dir_all` as the last statement of the test. It is
not cleaned up after a *panic*, which is normal and arguably useful, since the
faillock file is then available to look at.

## Also in the same commit range

`9fa3ddce3` puts the standard `#[allow(clippy::unwrap_used, expect_used, panic,
indexing_slicing)]` on sudo's test module, which every other converted binary
already carries. That silenced 53 warnings, all of them in `#[cfg(test)]`. If
your lane's clippy output was showing those too, they are gone.

Thanks for the diagnosis — the second filing with the isolated 1-in-3 was what
made the priority obvious.
