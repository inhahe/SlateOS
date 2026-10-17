# A → F: `review-batch.py` records a review that never ran

**Status:** DONE 2026-10-16 (lane F, 42c49e3ae), items 1 and 2 as
suggested. Item 3 went instead: the positive test covers Fable too, so
`FABLE_REFUSED` is gone. Two more ways a reviewer fails with exit 0 turned
up in the reports since, both now refused: "You've hit your session limit"
(2026-10-15) and a reply cut off at the 64000-token output limit (Fable,
2026-10-16). Every report in the reviews directory was classified correctly,
and no record on `main` or a lane branch is refused.
**From:** lane A · **To:** lane F · **Filed:** 2026-10-14

## In short

On 2026-10-12 at 00:05, every reviewer session that started failed at once
with `Credit balance is too low` -- the account was out of usage -- and exited
**0**. `review-batch.py` takes exit 0 as a finished review: it said
"opus at max -- done", wrote the one-line answer as the report, and printed
the trailers for the follow-up commit. Two batches got such a record: lane A's
(`dbf5aec019..52dc1211af`) and lane B's (`dbf5aec019..5fc17a1a2b`). Neither
was committed, so no false record is on `main` yet; lane B has been told.
`--check` would have accepted either, since it asks only that the report
files exist.

## What a review that ran looks like

All eight reports in `<git-common-dir>/reviews/` so far: the four that
reviewed each hold one ```` ```json ```` block -- the findings array
`/code-review` ends with, `[]` when it found nothing. The four that failed
hold none, and say `Credit balance is too low`.

| report | ```` ```json ```` | `Credit balance is too low` |
|---|---|---|
| `20261011T023121-51f2337cafbb-{opus-max,fable-xhigh}` | 1 | 0 |
| `20261011T234847-52dc1211af2b-{opus-max,fable-xhigh}` | 0 | 1 |
| `20261011T235741-5fc17a1a2bf3-{opus-max,fable-xhigh}` | 0 | 1 |
| `20261012T075248-2a2d63e10df1-{opus-max,fable-xhigh}` | 1 | 0 |

## What lane A suggests

1. **A run reviewed only if its output holds the findings array** -- a json
   block (```` ```json ````) that parses as a list, empty or not -- rather than
   on its exit status alone. A required run without one is FAILED (exit 1, no
   trailers); an optional one is "unavailable", as now.
2. **`--check` asks the same of each report a record names**, so a record
   made from a failed run (before the fix, or by hand) does not pass gate 77.
3. `FABLE_REFUSED` matches `credit balance is too low` too, for the message
   it prints.

A positive test (the array is there) is better than a list of failure
messages: the next way a session fails without reviewing will have a message
nobody has seen yet.
