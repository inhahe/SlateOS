### TD-TOOLS-A-CORPUS-CASE-RACED-A-BACKGROUND-JOBS-LIFETIME-AGAINST-A-PROCESS-SPAWN. `jobs-listing` failed once in a full sweep and passed alone — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/tests/corpus/jobs-listing.sh`, the last two groups of
the "…but only if the listing actually reported it" section.

**What.** One line of 456 differed, and only in the marker column:

```text
bash: [1]-  Done                    sleep 0.05
osh : [1]   Done                    sleep 0.05
```

**Not a shell bug.** The `-` marks the job that was current when the *next* job
was spawned, and both shells decide that by looking at the table as it stands at
that moment. bash's `set_current_job` (`jobs.c`) tries the old current job, then
any stopped job, then falls back to `job_last_running` — which skips a job
already reaped. So a background job that dies *before* the next one is spawned
earns no marker, in either shell. The case wrote

```sh
sleep 0.05 & sleep 0.7 & sleep 0.4; jobs %2; jobs
```

which asks a 50 ms job to outlive a process spawn. On an idle machine it does;
under a full sweep's load on Windows it does not, and osh lost the race bash won.

**Fixed** by giving those two groups the long-short lifetimes the case's own
header already prescribes and had already applied to four other sections —
`sleep 0.6 & sleep 2 & sleep 1.0` — so job 1 comfortably outlives job 2's spawn
and is comfortably finished before `jobs` looks. The header now says *every*
section that reads a `-` off a finished job takes that treatment, and cites
bash's fallback as the reason both shells are racy in the same place.

**Same class** as TD-TOOLS-A-CORPUS-CASE-RACED-TWO-PIPELINE-STAGES-FOR-THE-SAME-FD
and as the `jobs_marks_the_current_and_previous_job` unit test fixed the same day
(commit `7efa50de9`), which had the identical defect in Rust: a wall-clock
duration standing in for a fact that has to be arranged. Every such case wants
the fact made true by construction — a job long enough that nothing can outrun
it, ended by an explicit `kill` or `wait` — not by a duration that is merely
usually long enough.
