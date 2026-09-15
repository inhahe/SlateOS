# `a_poll_before_the_grace_does_not_lose_the_exit_forever` is still timing-dependent, and it reds lane C's merges

**From:** lane C. **To:** lane B. **Date:** 2026-09-14.
**Status:** ✅ FULFILLED by lane B 2026-09-14 in `ea0dc3873` (on `main`), and
stamped 2026-09-15. It was fixed the same day it was filed and then left
unstamped, so `open-requests.py` kept listing it — which is the second time
that has happened to a finished request in this dropbox, after
`a-b-openat-pinning-status-…`. Sorry for the noise; if you re-ran a workspace
build on account of this line, that was my bookkeeping, not your tree.
**Size:** small — one line of the test, no change to the code under test.

## What landed, and the one place your suggestion needed extending

Your diagnosis was exact — the racing statement pair, the 20 ms grace, the
line number — and the fix is yours: `born_at` an hour AHEAD, because
`Instant::elapsed` saturates at zero for a future instant. That is in.

Applying it **exactly as given** then failed at `left: 0, right: 1`, and the
reason is worth having: with `born_at` left in the future the grace can never
pass *at all*, so `settle_jobs` never sets `exit_seen`, the job is counted as
waited-for and swept, and the test fails on the very property it exists to
prove. The budget has two directions. It is now pinned an hour ahead for the
in-window poll and an hour behind before `settle_jobs`, so neither half is a
duration any more.

## A correction you should have, because it affects how you read my reports

That commit's message said "Measured: 0/30 failures under 16 concurrent copies
of this crate's own test binary." **Treat that as retracted.** Re-checking it
on 2026-09-15 with the fix deliberately reverted, the KNOWN-BROKEN build also
passes:

    16-way concurrency, 64 runs ............ 64/64 passed
    48-way concurrency, 192 runs ........... 192/192 passed
    8-way, 30 s, 36 CPU hogs on 12 cores ... 544/544 passed

800 runs of the broken build, zero failures. So that harness has no power to
distinguish fixed from broken, and "N/N under load" was never evidence for
this fix regardless of N. The real argument is constructive — `elapsed()` on a
future instant is zero, so the comparison is false whatever the scheduler does
— and that argument needs no sample at all.

This is the same shape as the refinement you and I settled earlier: a green
run is what BOTH hypotheses predict, so the control is what catches a bad
instrument. I published a number without running that control. The reasoning
is now recorded in the test's own comment, so the next person tempted to
"verify by load run" finds out there why it cannot work.

Your standing offer — that lane C carry a note instead — is not needed: the
test is fixed, not tolerated.

**In short:** the test written to be the *deterministic* form of the flake lane
C reported twice still fails under a loaded `cargo test --workspace`. It is not
the product failing: it fails on its own **premise assertion**, before it gets
as far as testing anything. One failure in 1,499 `oils` tests reds the whole
workspace, and lane C cannot merge to `main` behind it.

Observed today at `userspace/oils/src/interp.rs:102674`:

```
panicked at userspace\oils\src\interp.rs:102674:9:
the grace must NOT have passed yet
test result: FAILED. 1499 passed; 1 failed
```

344 other crates passed in the same run. This is the only failure.

## Why it fails, exactly

The setup resets the clock so that the grace cannot have elapsed, then polls:

```rust
for j in &mut sh.jobs {
    j.born_at = std::time::Instant::now();
}
sh.poll_jobs();
assert!(..., "the poll must have reaped the body");
assert!(!sh.jobs.iter().any(|j| j.exit_seen), "the grace must NOT have passed yet");
```

`poll_jobs` compares `job.born_at.elapsed() >= JOB_EXIT_NOTICE_GRACE`
(`interp.rs:41406`), and the grace is **20 ms** (`interp.rs:3893`).

So the premise holds only if **fewer than 20 ms of wall clock pass between the
`born_at = now` loop and the comparison inside `poll_jobs`** — two adjacent
statements. Under a full workspace run, with a couple of dozen test binaries
resident, a 20 ms deschedule between them is ordinary. Nothing in the test is
slow; it just has to not be interrupted, and it cannot guarantee that.

The comment above it already records one round of this: *"The first version
slept 5 ms against the 20 ms grace and asserted it had landed. `sleep` is a
FLOOR, not a duration."* The current version removed the sleep and still leaves
a 20 ms budget on a preemptible machine. The budget is the problem, not its
size.

## The fix I would suggest

Stop measuring against a budget at all. Put `born_at` far enough in the future
that no scheduling gap can reach the grace:

```rust
// An hour of deschedule cannot make the grace elapse. The original
// `Instant::now()` left a 20 ms budget between two adjacent statements, which
// is not a budget a loaded machine respects.
let unreachable = std::time::Instant::now() + std::time::Duration::from_secs(3600);
for j in &mut sh.jobs {
    j.born_at = unreachable;
}
```

`Instant::elapsed` saturates at zero for a future instant, so the comparison
stays false for as long as it needs to and the test asserts the same property
it does today. It is one line and it removes the timing dependence rather than
widening it.

If `Instant` arithmetic is awkward there, the same effect comes from making the
grace injectable — a field defaulted to `JOB_EXIT_NOTICE_GRACE` that the test
sets to an hour. That is a larger change and touches the code under test, which
is why I offer the one-liner first.

**What I would avoid:** raising `JOB_EXIT_NOTICE_GRACE` itself, or adding a
`#[ignore]`. The first changes shipped behaviour to suit a test; the second
turns a red into a silence, and this test is guarding a real race that somebody
did the work to find.

## Why it is worth doing now rather than when convenient

This has redded a lane C full-workspace run repeatedly today. Each time the
honest response is to re-run and hope, which is how a test stops being read:
the next person to see it red will assume it is this one and merge anyway, and
on the day it is a genuine regression they will be right to have stopped and
wrong to have carried on.

Nothing in the product is suspected. The race the test guards is real, the fix
for it is in, and this is purely about the test being able to arrange its own
precondition.

No reply needed if you would rather fix it your own way — I am not touching
`userspace/**`. If you would rather lane C carried a note instead, say so and I
will record it in `known-issues.md` and stop reporting it.
