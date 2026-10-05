### TD-OILS-JOBS-INVISIBLE-IN-A-FORKED-CHILD. `jobs` sees an empty table inside a pipeline stage or a command substitution — ✅ RESOLVED — 2026-08-01

**Where:** `userspace/oils/src/interp.rs` — the clone used for a pipeline stage
and the one used for a command substitution did not carry `self.jobs` across.

**Reproduce:**

```sh
sleep 2 &
echo "--- plain";    jobs
echo "--- piped";    jobs | cat
echo "--- subshell"; ( jobs )
echo "--- cmdsub";   echo "[$(jobs)]"
wait
```

bash lists the job for the first, the pipe and the substitution, and prints
nothing for `( jobs )`; osh listed it only for the first. (The original write-up
of this entry had the subshell line wrong on both sides — see "the boundary"
below, which is what settles it.)

**Why.** bash forks for all of these, and a forked child inherits the parent's
job list — it is ordinary process memory. osh's stage/cmdsub clones are built
for *speed* on the hot path and copy only what a stage was thought to need, so
the job table was left out.

**The boundary.** Not every fork keeps its jobs. bash's `execute_in_subshell`
calls `without_job_control`, which empties the table, and that is the very same
boundary `$BASH_SUBSHELL` counts: a `( … )`, an `&` job, a coproc, and a
pipeline stage that is a compound command / function call / `eval`·`.`·`source`
all start with none, while a stage that is a plain command — and a `$( … )` —
keep the table. osh already encoded that distinction for `$BASH_SUBSHELL`
(`stage_pending_level` / `enter_stage_subshell`), so the fix needed no new
classification, only to hang the job table off the existing one.

**Fixed** by `Shell::inherit_jobs`, called from `clone_for_pipeline_stage` and
`new_comsub_shell`, with `enter_stage_subshell` clearing the table again for a
stage that turns out to be a subshell. The copy is a **snapshot, not a share**:
`Job::inherited_copy` carries everything a listing reads and marks the row
`inherited`, and the `child: Option<JobBody>` handle deliberately does not come
across — a process cannot wait for its parent's children, and that absence is
what shapes the rest.

Because it cannot wait, the first `wait` of any kind in such a child is where it
finds out: bash's `waitpid` answers `ECHILD` and bash responds with
`mark_all_jobs_as_dead`, so the whole inherited table is written off as finished
and a later `jobs` in that same child shows `Done` for jobs still running
perfectly well (`Shell::mark_inherited_dead`). Each `wait` form then answers its
own way — an operand-less one returns 0 without blocking, `wait -n` answers 0 and
takes a row away, and a targeted one hands back the -1 `wait_for` failed with.
`$?` is signed and prints that as written, while the *substitution's* own status
is an exit status and so eight bits wide, hence 255 (`forked_child_status`).

Covered by `userspace/oils/tests/corpus/jobs-in-a-forked-child.sh` (byte-exact
against bash 5.2) and four unit tests in `interp.rs`.

**Impact (before the fix).** `jobs` was wrong exactly where a script reads it
(`jobs | wc -l`, `n=$(jobs -p)`). Found while making a coproc a real job: the
probe piped `jobs` through `sed` to blank out pids and so saw nothing at all.

**Known deviations left.** Four, each measured and deliberately not emulated —
see TD-OILS-FORKED-CHILD-WAIT-N-CAPS-AT-TWO, TD-OILS-KILL-IN-A-FORK-DOES-NOT-
REACH-THE-PARENTS-JOB, TD-OILS-FORKED-CHILD-KEEPS-ITS-OWN-JOBS-ALIVE and
TD-OILS-KILL-THEN-JOBS-REPORTS-TERMINATED-NOT-RUNNING below.
