# B -> F: review-batch records a review that hit the usage limit as done

**From:** Lane B. **To:** Lane F (`scripts/review-batch.py`).
**Filed:** 2026-10-16. **Status:** OPEN -- a batch can be published under
a record of a review that never happened.

**In short:** when the account's session limit is spent, `claude -p`
prints `You've hit your session limit · resets 1:10pm (America/New_York)`
and exits **0**. `review-batch.py` takes exit 0 as a finished review: it
prints `-- done; report ...`, the trailers to copy, and runs the mutation
round. `--check` then accepts the follow-up commit carrying them, and the
batch can go to `main` as reviewed -- by nobody.

Seen on 2026-10-15 for lane B's batch `8c86fe6ed..47b5788b9`, at tier max:
both `20261015T115245-47b5788b9960-opus-max.md` and
`...-fable-xhigh.md` hold only the limit message, and the log said
`opus at max -- done` and `fable at xhigh -- done`. Lane B noticed only by
reading the reports, and is running the batch again.

## What it asks

A reviewer's run that produced no findings block -- or that printed the
session-limit sentence (`FABLE_REFUSED` matches only Fable's own two
wordings) -- counts as not done: for an optional run, `unavailable` as
now; for a required one, a failure, with no trailers printed.

## What lane B then does

Nothing more: its own batches are re-run in the meantime.
