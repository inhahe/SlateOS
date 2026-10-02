## B-INETD-RATE-LIMIT-WINDOW-WAS-MICROSECONDS-NOT-MINUTES (lane B) — FIXED 2026-09-05

**In short:** `inetd`'s per-source rate limiter — the thing that is supposed to
stop one machine opening a thousand connections a second to a service — counted
connections over a window it believed was sixty seconds but which was in fact
sixty *microseconds*. Nothing can be too fast for a sixty-microsecond window, so
every connection arrived to find the previous one already forgotten, the
measured rate never rose above one, and a `MaxRate` limit of any value was never
once exceeded. The daemon logged nothing and rejected nobody: the limit was
configured, displayed, parsed, tested — and dead.

This is the second instance of the same defect found in the same hour; the first
is `B-SSHD-LOGIN-GRACE-TIMER-COMPARED-MICROSECONDS-AGAINST-SECONDS` above. They
were written independently and share no code, which is the point of recording
both.

### Where

`userspace/inetd/src/main.rs`. The window was a bare literal in
`SourceTracker::prune`:

```rust
/// Prune timestamps older than 60 seconds from the rate window.
fn prune(&mut self, now_ms: u64) {
    let cutoff = now_ms.saturating_sub(60_000);
    self.recent_timestamps.retain(|&ts| ts >= cutoff);
}
```

`now_ms` came, through five call sites, from a wrapper named
`clock_monotonic_ms` that read `SYS_CLOCK_MONOTONIC` and returned it unchanged.
That syscall returns **nanoseconds since boot** (`kernel/src/syscall/number.rs`;
host shim `posix::syscall::host_clock::monotonic_ns`). So `60_000` was 60 µs.

### Why nothing caught it

Three separate reasons, each individually reasonable:

- **The unit lived in a name, and the name was wrong.** Every use of `now_ms`
  reads correctly if you accept the name. `saturating_sub(60_000)` against a
  millisecond clock *is* sixty seconds.
- **The tests were written against the same wrong unit.** `prune` had a test —
  `test_source_tracker_prune_old_timestamps`, with timestamps 1000/50 000/70 000
  and a prune at 70 000 — and it passed, because it asserted the arithmetic of a
  `60_000` window rather than asserting that the window was a minute. A test
  that restates the implementation's constant cannot detect that the constant is
  wrong. This is the more interesting failure of the two: the code had coverage,
  and the coverage was of the wrong proposition.
- **No host test could reach the real clock.** `inetd` defines its own
  `syscall0` whose off-target stub returns `-38` (ENOSYS) for everything, so on
  the dev machine `clock_monotonic_*` answers 0 and the tests must pass their
  own timestamps in. Nothing on the host ever observes the true unit.

Note that `sleep_ms` immediately above the clock wrapper already carries a
comment about an identical unit bug against `SYS_SLEEP`, fixed earlier. The
comment was right there and the same mistake was made again ten lines below it,
which is the argument for a shared accessor rather than a per-binary wrapper.

### The fix

`clock_monotonic_ns`, named for what it returns; a `NANOS_PER_SEC` constant; and
the window promoted from a literal to `const RATE_WINDOW_NS: u64 = 60 *
NANOS_PER_SEC` with a doc comment saying which configuration key it implements.
The sixteen `now_ms` parameters became `now_ns`.

The three tests that had encoded the old window (`prune_old_timestamps`,
`garbage_collect`, `rate_expires_after_window`) were rewritten in terms of
`RATE_WINDOW_NS`, so they now test the *pruning* and move with the constant
instead of pinning it.

Pinning the constant is a separate test, and a behavioural one rather than an
`assert_eq!` restating the definition:
`test_rate_window_is_one_minute_of_the_monotonic_clock` records ten connections
spread over one second and asserts the rate is ten. Under the old window it
reports `left: 1, right: 10` — which is precisely the production symptom, not a
proxy for it.

### The general lesson

Beyond the unit lesson from the sshd entry: **a test that reuses the
implementation's constant tests only self-consistency.** `prune`'s test would
have passed for any window whatsoever. Where a constant encodes a claim about
the outside world — "a minute", "a gigabyte", "the MTU" — at least one test must
assert the claim in the outside world's terms (ten connections in one second are
inside a one-minute window) rather than in the constant's.
