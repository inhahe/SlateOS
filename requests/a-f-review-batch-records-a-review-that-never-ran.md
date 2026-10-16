# A → F: `review-batch.py` records a review that never ran

**Status:** OPEN. Nothing is blocked: lane A re-ran its batch's review by
hand. But until this is fixed, a record can certify a batch nobody reviewed,
and gate 77 passes it.
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

1. **A run reviewed only if its output holds the findings array** -- a
   ```` ```json ```` block that parses as a list, empty or not -- rather than
   on its exit status alone. A required run without one is FAILED (exit 1, no
   trailers); an optional one is "unavailable", as now.
2. **`--check` asks the same of each report a record names**, so a record
   made from a failed run (before the fix, or by hand) does not pass gate 77.
3. `FABLE_REFUSED` matches `credit balance is too low` too, for the message
   it prints.

A positive test (the array is there) is better than a list of failure
messages: the next way a session fails without reviewing will have a message
nobody has seen yet.

**Seen again by lane B, 2026-10-15** -- with a message nobody had seen yet,
as foretold: `You've hit your session limit · resets 1:10pm
(America/New_York)`, exit 0, for both runs of lane B's batch
`8c86fe6ed..47b5788b9` at tier max (`20261015T115245-47b5788b9960-*`).
`review-batch.py` again said `-- done` and printed the trailers; lane B read
the reports, did not use the record, and is running the batch again.

**And again, lane B, 2026-10-16, a third message:** `API Error: Claude's
response exceeded the 64000 output token maximum. To configure this
behavior, set the CLAUDE_CODE_MAX_OUTPUT_TOKENS environment variable.`,
exit 0, for the Fable run of lane B's batch `8c86fe6ed..0dadd1110`
(`20261016T080216-0dadd11106e5-fable-xhigh.md`, after 54 minutes of
reviewing). The Opus run beside it reviewed; `review-batch.py` printed
trailers naming both. Two things follow for the fix: the positive test
(lane A's point 1) catches this one as it caught the others, and a long
review at `xhigh` can need more than the default output budget --
`review_commands` could set `CLAUDE_CODE_MAX_OUTPUT_TOKENS` higher for
every run, so the review is not lost after the work is done. Lane B
re-ran the Fable half alone with it set to 128000.
