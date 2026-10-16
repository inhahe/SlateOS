## TD-F-REVIEWERS-USAGE-IS-MISSING-FROM-THE-USAGE-NUMBERS (lane F, 2026-10-16) — OPEN
**Status:** OPEN

**In short:** since `scripts/review-batch.py` runs its reviewers with
`--no-session-persistence` (ea8689e44, as `claude-global.md`'s review command
asks), a review leaves no transcript. The operator's usage analyzer
(`claude-usage-analysis`) builds its numbers from transcripts, so every review
-- an Opus `max` and a Fable `xhigh` run can take an hour -- is missing from
them, while the account's weekly meter still rises by what the review cost.

**What it skews:** `scripts/process-review.py`'s `meter_fit` pairs each window's
meter rise with the dollars of the calls the analyzer saw in it. Windows with a
review in them show a rise with too few dollars, which tilts the fitted weight
of cache reads, and with it the report's "cache reads are about N% of what
fills the weekly limit". The analyzer's own usage totals are low by the
reviews' share too. How much depends on that share of each account's week;
nothing measures it yet.

**Found by** the Opus `max` review of `d37e688ca..ea8689e44` (finding 3).

**The fix:** keep `--no-session-persistence` (the sessions it stops would
clutter the operator's session lists), and record each reviewer's usage
another way: run it with `--output-format json`, which ends with
`total_cost_usd`, `usage` and `modelUsage`; keep that in the review's report
beside the findings (`findings_of` then reads the `result` field); and have
`process-review.py` add each report's calls, per account (the report's header
names the account, `CLAUDE_CONFIG_DIR`) and per window, to what the analyzer
saw. `--check`'s reading of old reports must keep working.
