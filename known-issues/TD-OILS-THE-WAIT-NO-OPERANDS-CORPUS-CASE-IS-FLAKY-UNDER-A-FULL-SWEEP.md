### [B] TD-OILS-THE-WAIT-NO-OPERANDS-CORPUS-CASE-IS-FLAKY-UNDER-A-FULL-SWEEP. The job holding `$!` is not spared, once per many sweeps — 2026-08-14 — OPEN

**Where:** `userspace/oils/tests/corpus/wait-with-no-operands-and-a-job-that-just-ended.sh`,
the group "only the last one backgrounded is spared", against
`Shell::builtin_wait`'s operand-less arm and `Shell::drain_jobs`
(`userspace/oils/src/interp.rs`).

**What — and this time the whole row was captured.** One full
`scripts/osh-bash-diff.py` sweep came back `654 matched, 0 waived, 1 failed`
with **one line** of the case different, everything else in it identical:

```sh
( exit 3 ) & ( exit 4 ) & sleep 0.4; wait; echo "  noargs=$?"
VAR=stale; wait -n -p VAR; echo "  n=$? $(pvar)"
```

| | bash 5.2.37 | osh (this sweep) |
|---|---|---|
| `noargs=` | 0 | 0 (agreed) |
| `n=` | `4 a pid` | **`127 unset`** |

So osh had nothing left to report where bash still had the last-backgrounded
job. Re-run on its own immediately after: `1 matched, 0 waived, 0 failed`.
Saved report:
`target/dvscratch/corpus-failures/20260814-145703/wait-with-no-operands-and-a-job-that-just-ended.txt`.

**What a 127 requires, read out of the code rather than guessed.** The spare is
`builtin_wait`'s operand-less arm: after `drain_jobs`, every job with a status
is marked `notified` *except* the one whose pid is `last_bg_pid`, and
`cleanup_dead_jobs` then drops exactly the notified ones. But `drain_jobs`
itself marks `notified` for every job it *waited for*, and it waits for any job
not already in its `known` snapshot — `known` being the jobs whose `exit_seen`
was set **before** the wait was reached. So the spare survives only when the
`$!` job's `exit_seen` was already set, which the unit-boundary
`cleanup_dead_jobs` does for a job that is both finished and older than
`JOB_EXIT_NOTICE_GRACE` (20 ms). A 127 means that did not happen for the `$!`
job specifically: had it been the *other* job that was late, `drain_jobs` would
have waited that one and the spare would still stand.

**The margin is not thin, which is what makes this odd.** Both shells were
measured at four margins (`build/pgS.sh`), and they agree exactly:

| `sleep` before the `wait` | bash | osh |
|---|---|---|
| none | `127 unset` | `127 unset` |
| 0.01 | `4 a pid` | `4 a pid` |
| 0.05 | `4 a pid` | `4 a pid` |
| 0.4 | `4 a pid` | `4 a pid` |

The flip is between 0 and 0.01, so the case's `sleep 0.4` is a ~40x margin — not
the ~1x margin that
TD-OILS-THE-COMPGEN-JOB-CORPUS-CASE-IS-FLAKY-UNDER-A-FULL-SWEEP turned out to
be. **Do not assume the same diagnosis and just widen the sleep.**

**Loads that do NOT reproduce it — do not spend the time again.** The job is
thread-backed, not a process (`( exit 4 ) & echo $!` prints the synthetic
`900000`, where `sleep 0.4 &` prints a real pid), so both of the obvious
starvation stories were tried and neither bit:

- 20 serial runs of the group alone: clean.
- 119 runs of the group at 8-way concurrency: clean.
- 64 runs of the *whole case* at 8-way concurrency: clean.
- 36 runs under a process-spawn storm (6 loops spawning `osh -c :` and
  `bash -c :` back to back, this host's documented ~200-290 ms spike source):
  clean.
- 30 runs under CPU saturation (24 busy-loop processes on 12 cores): clean.

Probes are `build/repro-wait.sh` (the group), `build/repro-wait2.sh` (the whole
case), `build/spawnstorm.sh`, `build/cpuburn.py` — all in the gitignored
`build/`, so re-create them from this entry if they are gone.

**Proper fix.** Unknown, and deliberately not guessed at. The next sighting
should establish which of the two conditions failed — whether the `$!` job's
body was genuinely unfinished at the unit-boundary poll, or whether the poll did
not run — by instrumenting `poll_jobs` to record, per job, `is_finished` and
`born_at.elapsed()` at each call, and dumping that when `wait -n` answers 127.
That distinguishes "the thread really was 400 ms late" from a bookkeeping fault,
and only the first is a case-margin problem.

**Impact.** An intermittently red sweep, which is the gate on every commit —
and the sweep takes ~19 minutes, so a re-run to disambiguate is expensive.

**Sighting 2026-08-14, in the *unit* suite, and fixed there.**
`interp::tests::wait_n_ignores_a_job_whose_status_was_already_reported` failed
once under `cargo test -p oils --lib` (`wait -n` answered 127 where 3 was due,
i.e. the operand-less `wait` had *not* spared the job) and passed when re-run
alone. Same shape as this entry, but with a cause the test owned: it backgrounded
`( exit 3 ) &` and then slept a constant `0.2` to make the job finish first, and
no constant is long enough to promise that on a loaded machine. Fixed properly
rather than by lengthening the sleep — a new `settle_jobs` test helper (the
whole-table form of the existing `settle_job`) polls `poll_jobs` until every job
has a status, after the same `JOB_EXIT_NOTICE_GRACE`. That removes this test from
the flaky family; the *corpus* case above is untouched and stays open.
