### TD-OILS-KILL-IN-A-FORK-DOES-NOT-REACH-THE-PARENTS-JOB. `kill %1` inside `$( … )` names an inherited row but signals nothing — OPEN — 2026-08-03

**Where:** `userspace/oils/src/interp.rs` — `builtin_kill`. An inherited row has
`child: None` by construction, and osh refuses to signal a pid it did not spawn.

**Reproduce:**

```sh
sleep 30 >/dev/null 2>&1 &
echo "$( kill %1; echo "rc=$?" )"
sleep 0.3
jobs
```

Both shells report `rc=0`. Under bash the job is gone by the `jobs`; under osh
it is still `Running`.

**Why.** bash's fork is a real process, so `kill` reaches a pid the kernel knows
is alive whoever asks. osh's `$( … )` is an in-process clone: the inherited row
carries no `JobBody`, and signalling a bare pid would need a process-control
layer osh does not have on the host (the same gap as
TD-OILS-KILL-BY-PID-NEEDS-PROCESS-CONTROL). The *status* matches; only the
effect is missing.

**Proper fix.** Signal by pid through the host's process-control API, which
would also let `kill` reach a pid the shell never spawned. Until then a fork
cannot kill its parent's jobs.

**Impact.** A script that kills a job from inside a substitution or a pipeline
stage rather than from the shell itself. Rare, but silent when it bites.
