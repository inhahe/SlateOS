## TD-C-A-LANE-B-TEST-REDS-LANE-C-S-WORKSPACE-RUNS -- FIXED 2026-09-14 by lane B

**Date:** 2026-09-14. **Lane:** C (the report; the test is lane B's). **OPEN.**

**In short:** one test in `userspace/oils` fails under a loaded full-workspace
run, and one failure out of 1,499 reds the whole thing, so lane C cannot merge
behind it. It is not the product failing -- the test cannot arrange its own
starting conditions when the machine is busy.

`interp::tests::a_poll_before_the_grace_does_not_lose_the_exit_forever`,
`userspace/oils/src/interp.rs:102674`, on the assertion *"the grace must NOT
have passed yet"* -- a **premise** check, before it tests anything.

**FIXED by lane B** (`ea0dc3873`, merged as `a76cf245c`): `born_at` is pinned
an hour ahead for the in-window poll and an hour behind before `settle_jobs`.
0 failures in 30 runs under 16 concurrent copies of the test binary, and it
still catches the defect it exists for.

**My suggested fix was half of one, and the half I could not see is the
instructive part.** I proposed only the forward pin. Lane B applied it exactly
as given and the test then failed at `left: 0, right: 1` -- because with
`born_at` an hour in the future the grace can *never* pass, so `settle_jobs`
never sets `exit_seen`, the job is counted as waited-for, swept, and the
listing comes back empty. The test would have failed on the very property it
exists to prove.

The budget has two directions and I only looked at one. I had read the window
as "do not let the grace elapse yet" when it is really "do not let it elapse
here, and do let it elapse there" -- a shape I would not have found without
running it, which lane B did and I did not.

**Measured 2026-09-14, and it is not occasional:**

| run | result |
|---|---|
| `cargo test --workspace` | FAILED (1 of 1,499 in `oils`; 344 other crates green) |
| `cargo test --workspace`, again | FAILED, same test, same assertion |
| `cargo test -p oils` alone | **ok, 1,500 passed** |

So it is reliable under workspace load and absent in isolation -- which is what
the test's own comment predicts ("0 in 250 runs in isolation"). Lane C's
changes cannot reach it: `oils` is `userspace/`, depends on nothing lane C
touched, and the only coupling is total machine load.

**Why.** The setup sets every job's `born_at` to `Instant::now()` and then
calls `poll_jobs`, which compares `born_at.elapsed() >= JOB_EXIT_NOTICE_GRACE`
(20 ms). The premise holds only if fewer than 20 ms pass between two adjacent
statements. Under a workspace run with dozens of test binaries resident, that
deschedule is ordinary.

The test's own comment records an earlier round of the same thing -- a 5 ms
sleep against the 20 ms grace, with the note that *"`sleep` is a FLOOR, not a
duration"*. The budget is the problem, not its size.

**Not lane C's to fix** (`userspace/**` is lane B's), so it is filed as
`requests/c-b-the-deterministic-oils-job-test-is-still-timing-dependent-and-it-blocks-merges.md`
with a one-line suggestion: set `born_at` an hour in the future instead of now,
since `Instant::elapsed` saturates at zero and no deschedule can then reach the
grace.

**Why this is recorded here and not merely re-run.** Because re-running is the
honest response *today* and the corrosive one by next week. The next person to
see this red will assume it is this test and merge anyway -- and on the day it
is a genuine regression they will be right to have stopped and wrong to have
carried on. A flake nobody writes down becomes a red nobody reads.

**Do not "fix" it by raising `JOB_EXIT_NOTICE_GRACE` or adding `#[ignore]`.**
The first changes shipped behaviour to suit a test. The second turns a red into
a silence, and the race this test guards is real -- somebody did the work to
find it, and the test is the only thing standing over it.
