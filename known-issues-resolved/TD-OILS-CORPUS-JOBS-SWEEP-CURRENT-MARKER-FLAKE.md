### TD-OILS-CORPUS-JOBS-SWEEP-CURRENT-MARKER-FLAKE. `tests/corpus/jobs-sweep-line.sh` disagreed once on the `+` current-job marker — 2026-08-05 — ✅ FIXED 2026-08-06 (test bug: bash's job-birth race, mechanism measured and read out of `jobs.c`)

**Where:** `userspace/oils/tests/corpus/jobs-sweep-line.sh` lines 13–17, the
`== …but not the line itself` section:

```sh
echo "== …but not the line itself"
true & sleep 0.2
jobs
jobs %1
echo "rc=$?"
```

**Symptom.** One full `scripts/osh-bash-diff.py` run reported
`381 matched, 0 waived, 1 failed`, the only differing line being the `jobs`
listing: bash printed `[1]   Done                    true` (no marker) where osh
printed `[1]+  Done                    true`. Nothing else in the ~40-line
output differed — same rows, same statuses, same exit codes. The case was then
re-run in isolation six times and matched every time, so the capture we have is
the diff summary rather than a reproducible case.

**The mechanism, now read out of the source rather than guessed at.** The
earlier guess in this entry — that `reset_current()` runs when job 1 is *reaped*
— was wrong, and the source says why: `waitchld` only reaches its
`reset_current ()` when `set_job_status_and_cleanup` returns non-zero, and that
function increments `call_set_current` **only** on a stop or a
stopped→running resume (`jobs.c`), never on a plain death. A job merely dying
therefore never moves the markers. Nor does the foreground `sleep`'s row
leaving: `delete_job` calls `reset_current` only `if (job_index ==
js.j_current || job_index == js.j_previous)`, and a foreground job is never
either — only `stop_pipeline`'s async branch sets the current job.

The real window is at the job's **birth**, not its death. `stop_pipeline`
(`jobs.c`) builds the row from the child's *already-recorded* status —
`waitchld` writes `child->status` / `child->running` before it checks whether
the child has a job yet, and `continue`s if it does not — and then:

```c
newjob->state = any_running ? JRUNNING : (any_stopped ? JSTOPPED : JDEAD);
...
if (async) { ...; reset_current (); }
```

So if the async child exits and its `SIGCHLD` is delivered in the gap between
the fork and `stop_pipeline`'s `BLOCK_CHILD`, the job is born **JDEAD**. The
`reset_current ()` on the next line then finds no stopped job and no running one
(`job_last_running` skips the dead row), falls through to `js.j_current =
js.j_previous = NO_JOB`, and *every* row prints unmarked. The row itself is
still listed — JDEAD is not deleted, only `cleanup_dead_jobs` deletes — so the
only thing the race changes is the marker. That is exactly the captured
`[1]   Done                    true`.

**Measured.** The section was run against bash alone 1900 times (400 + 1500);
all 1900 printed `[1]+`. The window is real but very narrow — consistent with
one occurrence across hundreds of full sweeps. osh is the shell that is right
here: it decides the current job from the table, so an instant exit cannot
un-mark a row.

**Fixed** by removing the window from the case rather than by relaxing an
assertion, so the sweep coverage is untouched: every job in the file is now
`sleep 0.05 &` instead of `true &` (and `( sleep 0.05; exit 5 ) &` for the one
that carries a status). The child is now guaranteed to still be alive while its
own row is being built, so `any_running` is true and the job can never be born
JDEAD. The header records the mechanism.

Widening the job's lifetime tightens the margin at the *other* end, though, and
the first attempt kept the original 0.2 s foreground gap and was caught by it:
run back-to-back with 1500 concurrent bash spawns saturating the machine, the
case failed once (`sleep 0.05` had not finished by the time `jobs` looked), then
passed 7 times in a row once the machine was idle. Both halves of "is the job
finished yet" cost a process spawn, and a spawn under load can run to a large
fraction of a second, so the gap is now `sleep 0.5` — ~450 ms of slack in the
direction that matters. Re-validated 4× *while* a 900-spawn load generator was
running, plus `jobs-listing` 2× under the same load; all green.

**Also fixed alongside, same root cause, opposite marker.** A later sweep caught
`tests/corpus/jobs-listing.sh` disagreeing on the `-` *previous*-job marker
(bash `[1]-  Done`, osh `[1]   Done`, twice). Same family: `set_current_job`
picks `js.j_previous` with `candidate = RUNNING (js.j_current) ?
job_last_running (js.j_current) : job_last_running (js.j_jobslots);`, so a job
earns `-` **iff it was still RUNNING at the moment the next job was spawned** —
which a 0.05 s lifetime decides by race against a process spawn. Retimed: the
two `sleep 0.05 & sleep 0.8 & …` lines whose marker depends on that became
`sleep 0.6 & sleep 2 & sleep 1.0; jobs`, and the three "reaping a job" lines,
which want the *opposite* fact (job 1 already dead when job 2 spawns), got an
explicit `sleep 0.4` gap instead of relying on a short lifetime. The case
already states a `# TIMEOUT: 60` budget; it runs in ~25 s per shell.
