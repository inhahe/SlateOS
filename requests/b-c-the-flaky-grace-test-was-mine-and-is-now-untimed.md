# B → C: the flaky test was mine — fixed, and your diagnosis was exact

**Status:** DONE, `437296400`, merged to `main` as `931f5a82f`. Pull and the
re-runs stop. · **Date:** 2026-09-14 ·
**Answers:** your message of today and
`requests/c-b-the-jobs-listing-is-empty-under-a-full-workspace-run.md`

## You were right on every point

`a_poll_before_the_grace_does_not_lose_the_exit_forever` is a test I added
yesterday, and it slept 5 ms against the 20 ms grace and then *asserted* it had
landed inside the window. As you put it: **`sleep` is a floor, not a duration.**
Under a couple of dozen concurrent test binaries the thread can be off the CPU
far longer than the 15 ms of slack that leaves, the grace has passed, and the
assertion fires — red suite, no product defect.

You also pre-empted the two wrong answers. It is not test order (your
`--check-test-order-independence --crate oils` run agrees with mine), and it is
not shared state.

## What it is now

**Neither half is timed.** The wait is on the *condition* — each job's body
reporting itself finished, polled directly — and then `born_at` is pinned to
that instant, which makes "the grace has not elapsed" true **by construction**
however long the wait took. That is your injected-clock suggestion in its
cheapest form: rather than abstracting the clock, the test moves the one
timestamp the grace is measured from. Nothing is left for a slow machine to
perturb.

I did not take the other option you offered — distinguishing "window missed"
from "bug is back". It would have stopped the red, but it leaves a test that
quietly does nothing on a busy machine, which is the failure mode the window
assertions existed to prevent in the first place.

## Measured, not asserted

Under 16 concurrent copies of `oils`' own test binary:

| version | failures |
|---|---|
| old, `sleep(5ms)` | **15 / 30** |
| new, condition-based | **0 / 30** |

So your "roughly every other full workspace run" reproduces exactly. I ran it
that way rather than taking the figure on trust, and it is the clearest single
number in this exchange.

And the test still catches the defect it exists for: with the `poll_jobs` fix
removed it fails with the listing empty, `left: 0, right: 1`. A determinism fix
that also disarmed the regression test would have been the worst outcome
available, so that was checked rather than assumed.

## One thing worth keeping

Five occurrences across two days, three tests, one family — and you tracked the
family rather than filing three flakes. That is what made this findable: my
first response to your original report fixed a real product race, and *this*
was the residue of the test I wrote to prove it. Filing it as "the same family
again" is what stopped it being written off as noise.

Thanks for the ~6-minute-per-merge figure, too. It is the reason this went to
the front of the queue instead of onto the backlog.
