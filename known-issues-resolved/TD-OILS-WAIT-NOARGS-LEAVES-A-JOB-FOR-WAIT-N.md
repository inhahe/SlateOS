### TD-OILS-WAIT-NOARGS-LEAVES-A-JOB-FOR-WAIT-N. an argument-less `wait` sometimes does not consume the job it waited for — ✅ RESOLVED — 2026-08-01

**Where:** `userspace/oils/src/interp.rs` — the `wait` builtin's no-job-spec path
and the bookkeeping (`Job::notified` / the reaped-status memory) that decides
whether `wait -n` still considers a job pending.

**Reproduce:** intermittent; seen once in a full corpus run (`corpus19.log`),
not in 16 targeted attempts including eight concurrent runs under six spinning
CPU hogs. `tests/corpus/jobs-wait.sh`:

```sh
VAR=stale; ( exit 3 ) & wait -p VAR; echo "p-noargs=$? [${VAR-unset}]"
VAR=stale; wait -n -p VAR;           echo "p-nothing=$? [${VAR-unset}]"
```

| | bash | osh (failing run) |
|---|---|---|
| `p-noargs` | `0 [unset]` | `0 [unset]` |
| `p-nothing` | `127 [unset]` | `3 [900008]` |

**Diagnosis.** `900008` is the synthetic pid of the job started on the *first*
of those two lines — the ninth thread-backed background job in the file, counting
from 900000. So the argument-less `wait` on line 1 returned 0 without marking
that job consumed, and the `wait -n` on line 2 then reported it instead of
answering "no unwaited-for children". Both lines agreed with bash on the run's
other 60-odd checks, so the divergence is confined to the purge.

The intermittency points at *which* path the `wait` took: when the body has
already finished and been reaped by a sweep, `wait` answers from the remembered
status and purges; when it is still running, `wait` blocks on it, and it is that
branch that appears not to mark the job. A busy machine makes the second branch
likely, which is why only the full corpus run has hit it.

**Impact.** `wait; wait -n` reports a stale job's status where bash reports 127.
Narrow, but it is exactly the shape a script uses to drain a job pool and then
ask whether anything is left.

**Fixed** the same day, and the diagnosis above was wrong in an instructive way.
The blocking branch marks the job perfectly well; what varied was *which* branch
ran. `drain_jobs` splits the jobs into the ones it waited for and the ones it
found already over, and reads `Job::exit_seen` to tell them apart — after calling
`poll_jobs` to catch up first. That catch-up is the bug: bash learns of an exit
asynchronously, from a `SIGCHLD` that arrived while it was busy, so what it knew
when the `wait` was reached is what it had been *told*, not what it could find
out by asking now. osh's glance is instantaneous and free, so a body that ended a
microsecond earlier was filed as "already over" and spared.

Two changes; see design-decisions.md §98 for the reasoning and the measurements.

* `drain_jobs` records which jobs were `exit_seen` *before* its own catch-up
  poll, and counts everything else as waited for.
* `Job::born_at` — a job's exit is not *seen* until `JOB_EXIT_NOTICE_GRACE`
  (20 ms) after it started, however finished the body looks. bash's job is a
  forked child and hearing that it ended costs a fork, an exit and a signal, so
  `( exit 3 ) & wait` always waits; osh's is a thread that can be over first.
  The grace is measured in *time*, not in commands, because bash's is: four `:`
  in a row buy bash nothing, and neither does a twenty-iteration `for ((…))`
  loop, but a two-hundred-thousand-iteration one does — with no external command
  anywhere in it. Only `exit_seen` is held back, not the reap, so `jobs` and
  `wait` still report the status without delay.

Two further divergences fell out of the same probe and were fixed with it:

* `poll_jobs` now clears `Job::notified` when it reaps a job. A job that
  *changes* state is owed to the user again — bash's `waitchld` clears
  `J_NOTIFIED` on every status change for exactly this reason — so
  `( sleep .3; exit 3 ) & jobs; sleep .6; jobs` reports it `Running` and then
  `Exit 3`, where osh's first listing used to swallow the second.
* `wait -n` no longer says `no such job` about a row it can see but has already
  reported. Measured: `( exit 3 ) & p=$!; sleep .3; wait; jobs; wait -n $p` is a
  *silent* 127 in bash, where `wait -n 12345` names the pid. The difference is
  whether the row is still in the table, so spreading that same line over four
  lines — which lets bash sweep at each input-unit boundary — makes bash name the
  pid after all.

Covered by `tests/corpus/wait-with-no-operands-and-a-job-that-just-ended.sh`,
which pins both sides of the grace, the `$!`-sparing rule and the re-announcement
rule, and was byte-identical to bash on both streams over three consecutive runs
of each shell.
