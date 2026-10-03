### TD-OILS-THE-JOBS-CURRENT-AND-PREVIOUS-TEST-IS-FLAKY-UNDER-LOAD. `jobs_marks_the_current_and_previous_job` failed once in a contended run — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/interp.rs` — the
`jobs_marks_the_current_and_previous_job` unit test.

**What.** One `cargo test -p oils --lib` run reported `1356 passed; 1 failed`
with this test failing; the same binary passed it alone immediately afterwards,
and passed the full suite twice more (1357/1357). The failing run was the only
one executed *concurrently with a full corpus sweep* — 475 cases each forking
`osh` and `bash`, so the machine was saturated with short-lived processes.

**Why (suspected, not yet proven).** The test starts background jobs and checks
which is `+` and which is `-`. That ordering depends on the jobs' observed
states, so a job that is reaped — or not yet reaped — at a different moment than
the test assumes will flip the marks. Under load the window between spawning a
job and the shell noticing its exit widens by far more than the test's margin.
The assertion message was not captured; the next occurrence should be run with
`--nocapture` to record which mark landed where.

**Fixed by** removing every wall-clock assumption in both directions, which
turned out to need no new machinery — the test already had the primitive for one
direction and simply had to stop guessing at the other:

- *"This job has finished"* was already deterministic via `settle_job`, which
  polls until the shell **admits** the job is over rather than sleeping for a
  guessed interval.
- *"This job is still running"* had no such primitive and was expressed as
  `sleep 0.4 &` — a bet that the *next* job's spawn takes under 0.4 s, which on
  a saturated machine it does not. Every such job is now `sleep 30 &`, which no
  spawn can outlast, and is ended with an explicit `kill` instead of a `wait`.

That also made the test ~30× faster (4 s → 0.12 s): the long sleeps are never
waited for, whereas the short ones were.

A fifo the test releases was considered and rejected: it would make the job's
lifetime the test's to control, but osh is a native Windows binary and the fifo
would have to come from MSYS, which it cannot open. `sleep 30` + `kill` reaches
the same guarantee with nothing to port.

**Impact:** was test-only. No evidence of a product bug; the marks are correct in
every corpus case that exercises them.

**Sibling, fixed with it:** the same failure mode in the corpus —
TD-OILS-THE-JOBS-LIFETIME-CORPUS-CASE-IS-FLAKY-UNDER-LOAD, below. That one could
not use this remedy: a corpus case is a script both shells run, so it has no
access to `settle_job` and must express its gates in wall clock; there the fix
was to size the margins for a loaded machine.
