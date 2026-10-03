### TD-OILS-WAIT-N-REPORTED-JOB. `wait -n` re-reported a job whose status had already been announced — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/interp.rs` — `wait_next()`, the `candidates`
collection.

**Found by the same test flaking a second time.** A full `cargo test -p oils`
failed `interp::tests::wait_without_operands_names_no_job` at
`left: 3, right: 127` and passed in isolation — the identical signature to
TD-OILS-WAIT-NO-OPERANDS-FLAKE above, whose lesson is "an intermittent failure
is not evidence of a race; go find the deterministic reproducer". Applying that
lesson worked a second time. The question "what state does the failing run have
that the passing run does not?" pointed straight at *which branch* the
operand-less `wait` had taken, and forcing the already-finished branch with a
sleep plus an intervening `jobs` diverged every time:

```
( exit 3 ) & p=$!
sleep 0.3
wait          # rc 0 — spares the `$!` job from being marked reported
jobs          # announces it; the row lingers unswept
wait -n $p    # bash: `wait: <pid>: no such job`, rc 127.  osh: rc 3
wait -n       # bash: rc 127.  osh: rc 3
wait $p       # both: rc 5-style replay of the remembered status
```

**What.** `-n` answers with the *next* job to finish. A job whose death has
already been announced (bash's `J_NOTIFIED`, osh's `Job::notified`) has already
done that, so it is not a candidate — bash goes as far as denying it exists at
all. osh's `wait_next` collected *every* row in the table, so an unswept
notified row was re-reported: rc 3 where bash gives 127, and a status
successfully returned for a pid bash calls `no such job`. A row surviving the
announcement is there for a `jobs` or a `wait %1` to read once more, and for a
*targeted* `wait PID` to replay — never for `-n`.

**Why it looked like a race.** An operand-less `wait` marks every job it
*actually waited for* as reported, but `drain_jobs` polls first, so a job that
had already finished never enters `waited` and is spared. Whether a following
`-n` therefore found a stale unswept row came down to how fast the job's thread
ran — timing decided *visibility*, not correctness. The bug itself was fully
deterministic. Second data point for the lesson: this failure shape (a `wait`
returning a status where bash returns 127, intermittently) has now twice been a
real deterministic divergence rather than a race.

**Fix.** `wait_next` filters `notified` rows out of both candidate paths. The
bare path skips them; the operand path turns a `JobLookup::Found` on a notified
row into the same `wait: <spec>: no such job` diagnostic a `NotFound` gets, by
merging the two match arms.

**Tests.** `interp::tests::wait_n_ignores_a_job_whose_status_was_already_reported`
pins the spared-vs-announced split, the `no such job` wording, the surviving
targeted replay, and that a reported row does not shadow a live job. The
`spared-*` / `reported-*` / `shadow-*` block in `tests/corpus/jobs-wait.sh`
pins the same shapes against bash with sleeps chosen so the branch is decided by
the script, not the scheduler. `wait_without_operands_names_no_job` now sleeps
inside its job body for the same reason, which removes its timing dependence.
