### TD-OILS-CORPUS-JOBS-TIMEOUT. `tests/corpus/jobs-listing.sh` fails the differential harness under machine load — ✅ RESOLVED 2026-07-30 (flaky test, not a shell bug)

**Symptom.** A full `scripts/osh-bash-diff.py` run reported
`107 matched, 0 waived, 1 failed`, the failure being `jobs-listing` with osh's
stdout empty, its stderr `<timed out after 20s>` and `status: osh=-1`. Re-running
that one case passed immediately, and a second full run with nothing else on the
machine gave `108 matched, 0 failed`. The run that failed had a
`cargo clippy -p oils` compiling alongside it.

**Cause.** `jobs-listing.sh` is the corpus's most wall-clock-hungry case: it
starts more than twenty background jobs and its `sleep` durations alone (0.05 to
0.9 s each, plus a `sleep 5` killed twice) add up close to the harness's flat
20-second per-case limit. There is no headroom left for a loaded machine, so the
case is the first to tip over. Nothing about the *result* differs — the shell
does not misbehave, it just does not finish in time.

**Impact (before the fix).** A full-corpus run could report one spurious failure
when something else was compiling, and `jobs-lifetime` joined it once its own
race fix (TD-OILS-CORPUS-JOBS-LIFETIME-RACE) pushed it from 18.2 s to 21.1 s
combined.

**Fixed.** In `scripts/osh-bash-diff.py`, plus a header line in each of the two
job cases:

- **A case can declare its own budget** with a `# TIMEOUT: <seconds>` magic
  comment, parsed in `load_cases` alongside `# STDIN:`/`# EXPECT-DIFF:` and
  carried on the `Case` dataclass into `run_case`'s `subprocess.run`. The 20 s
  default is unchanged for the other 148 cases — raising it globally would have
  turned every genuine hang into a long stall, which is the thing the timeout
  exists to prevent. `jobs-listing.sh` and `jobs-lifetime.sh` ask for 60 s.
- **A timed-out case is retried once** (`measure()`, wrapping `run_case`). A
  timeout is not a result to compare — it says the *measurement* failed, and the
  cause here is machine load rather than the shell. The retry costs nothing on a
  healthy run and a case that blows its budget twice running is a real hang.
  `Run` grew a `timed_out` flag so the retry keys off the actual cause instead of
  pattern-matching the `<timed out after Ns>` stderr text.
- The sleeps were left alone. Shortening them was the other candidate, but their
  length is exactly what buys the case its immunity to scheduling jitter — the
  durations have to *order* the jobs reliably, and squeezing them would trade a
  load flake for a timing flake.
