### TD-OILS-JOB-DEATH-LEARNED-EAGERLY. osh notices a background job's death sooner than bash, which makes the `$!`-sparing rule of an operand-less `wait` nondeterministic — 2026-07-31 — MOSTLY ADDRESSED 2026-08-01 (design-decisions.md §98); a 20 ms residue remains

**Where:** `userspace/oils/src/interp.rs` — `Shell::poll_jobs` and its
callers, chiefly `notify_signalled_jobs` (run between every top-level
command, `exec_items`) and `drain_jobs` (the operand-less `wait`).

**Symptom.** A full `scripts/osh-bash-diff.py` run failed `jobs-wait`
about one time in three with `wait-spec=127` against bash's `0`. The job
the block probes had been given number **2**, because a finished-but-
unreported job was still holding number 1, so the closing `wait %1` found
nothing.

**Why.** An operand-less `wait` "spares" the job holding `$!` — it stays
in the table to be announced by a later `jobs` — but only if the shell
*already knew* that job was dead when the `wait` was reached; a job the
`wait` itself had to block on counts as waited-for and is swept. Both
shells implement that rule. They disagree about when the shell knows:

```sh
true & echo mid  ; wait; jobs   # both: empty      — nothing reaped it yet
true & sleep 0   ; wait; jobs   # both: [1]+ Done  — the fork/wait reaped it
```

osh's `poll_jobs` checks `JobBody::is_finished()` and is called from
`notify_signalled_jobs` between every command, so osh learns of the death
whatever ran in between — and, worse, *whether* the background thread had
finished by then is a race, so the same script spares on some runs and
sweeps on others. Twelve job/mid-command shapes were measured
(`target/dvscratch/jw/`) and bash is deterministic in all of them; osh
matches all twelve in isolation and diverges only under load.

**The fix first proposed here was wrong, and is superseded by
design-decisions.md §98.** This entry originally read bash as reaping a
background child *only* when it waits for a foreground one, and prescribed
"poll only where bash calls `waitchld`". The measurements taken for §98 the
next day refute that premise: with `true &` and no external command, no
`jobs`, no `kill` and no foreground child anywhere in between, bash still
spares the job after `for ((i=0;i<200000;i++)); do :; done` (~0.7 s) while
sweeping it after `for ((i=0;i<20;i++)); do :; done`. bash's `waitchld` runs
from the **SIGCHLD handler**, asynchronously — so learning between commands,
which is what osh does, is the *right* model. What separates the two rows is
elapsed time and nothing else.

**Addressed** by §98: `Job::born_at` plus `JOB_EXIT_NOTICE_GRACE` (20 ms) in
`poll_jobs`, which holds back only `exit_seen` (the shell's claim to have
*heard*) and not the reap, together with the `known` snapshot at the top of
`drain_jobs`. `tests/corpus/wait-with-no-operands-and-a-job-that-just-ended.sh`
pins it.

**Residue — why this entry stays open.** The 20 ms grace is a model of what a
fork costs, not a boundary either shell defines, so a script that backgrounds
an instantaneous job and then spends more than 20 ms in *builtins* before an
operand-less `wait` still has the job purged where bash might spare it. That
is a strictly narrower window than the one it replaced and it sits where
bash's own answer is a race, so it is not worth chasing further without a
measurement that shows it biting. Removing it at the root means spawning
background jobs as real processes — the much larger decision §97 declined.

**Worked around** in `tests/corpus/jobs-wait.sh` by reporting the table
once (`jobs >/dev/null 2>&1`) after the operand-less `wait`, which frees
job number 1 on both branches.
