## §98 — a background job's death is not news for the first 20 ms

**Date:** 2026-08-01
**Decided by:** Claude (autonomous)

An operand-less `wait` has to sort the jobs into two piles: the ones it *waited
for*, which it has therefore reported, and the ones it merely found already over,
which it has not. The distinction is observable — bash purges the first pile and
spares the `$!` job from the second, so a later `jobs` announces it — and it
turns entirely on whether the shell had been *told* of a job's death before the
`wait` was reached.

In bash that is a race the shell always wins, because hearing about a background
job costs a fork, an exit and a `SIGCHLD`. In osh a background job is a thread
that can be over before the `&` has finished being executed. So osh holds the
news back: `Job::born_at` records when the job started, and `poll_jobs` refuses
to set `exit_seen` until `JOB_EXIT_NOTICE_GRACE` (20 ms) has passed. The reap
itself is not delayed — `jobs` and `wait` report the status the instant it
exists; what waits is only the shell's claim to have *heard*.

**Why time, when §97 rejected a timer for the coproc.** Because here the thing
being modelled is itself a wall-clock quantity, and that was measured rather than
assumed. On the reference bash, with `true &` followed by various filler before a
`wait`:

| filler between the `&` and the `wait` | job survives the `wait`? |
|---|---|
| nothing | no — the `wait` waited for it |
| one to eight `:` | no |
| `for ((i=0;i<20;i++)); do :; done` | no |
| `for ((i=0;i<200000;i++)); do :; done` (~0.7 s, no external command) | **yes** |
| `sleep 0.05` | **yes** |
| `sleep 0.4` | **yes** |

No count of commands separates rows 3 and 4 — they are the same command — and no
"did an external process run" rule does either, since row 4 spawns nothing. Only
elapsed time does. §97's rejection of a timer stands for the coproc, where a
deterministic rule expressed in items *does* fit every measurement; it does not
generalise to a case where the measurements rule such a thing out.

**Choosing 20 ms.** The bounds above put bash's own boundary on this host
somewhere between "a twenty-iteration builtin loop" (tens of microseconds) and
`sleep 0.05`. 20 ms is an order of magnitude above any plausible run of builtins
and comfortably below the shortest delay bash was seen to notice within. It is a
model of what a fork costs, not a measured constant, and it is host-dependent in
bash too.

**Alternatives considered.**

- *No grace.* What was there before, and it made `tests/corpus/jobs-wait.sh`
  fail once in a full corpus run and never in sixteen targeted attempts — the
  worst kind of failure. See known-issues TD-OILS-WAIT-NOARGS-LEAVES-A-JOB-FOR-WAIT-N.
- *A grace counted in items, as §97 uses for the coproc.* Tried first; it is
  ruled out by rows 2–4 of the table above, which need opposite answers from
  equal or smaller command counts.
- *Only the `drain_jobs` snapshot* (record what was known before the `wait`'s own
  catch-up poll, and count everything else as waited for). Necessary but not
  sufficient: the sweep at the top of the `wait`'s own item already runs before
  `drain_jobs` and would have learned of the death there. Both changes are in.
- *Spawn background jobs as real processes.* Removes the divergence at the root,
  and is the same much larger decision §97 declined.

**The cost.** A script that backgrounds an instantaneous job and then does 20 ms
of *builtin* work before an operand-less `wait` will have the job purged where
bash — whose fork may well have been reaped by then — would spare it. That is a
narrower window than the one it replaces, and it is a region where bash's own
answer is a race.

**How to reverse.** Delete `Job::born_at`, `JOB_EXIT_NOTICE_GRACE` and the guard
in `Shell::poll_jobs`, and drop the `known` snapshot at the top of
`Shell::drain_jobs`. `tests/corpus/wait-with-no-operands-and-a-job-that-just-ended.sh`
pins the behaviour; the `settle_job` helper in `interp.rs`'s tests sleeps past the
grace and would no longer need to.
