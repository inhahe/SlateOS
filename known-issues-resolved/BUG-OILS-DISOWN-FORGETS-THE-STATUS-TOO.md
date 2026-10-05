### BUG-OILS-DISOWN-FORGETS-THE-STATUS-TOO. `wait PID` on a disowned job reported it as a stranger — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `Shell::disown_job`.

**What.** Found while auditing the row-dropping paths for TD-OILS-JOB-SWEEP-LINE
above. `disown` dropped the row and remembered nothing, so a later `wait $pid`
fell through to the "not a child of this shell" path:

```sh
( exit 5 ) & p=$!; sleep 0.3; disown %1
wait $p           # bash: rc=5   osh: osh: wait: pid N is not a child … / rc=127
( sleep 5 ) & q=$!; disown %1
wait $q           # bash: rc=0, at once   osh: rc=127
```

**✅ RESOLVED 2026-07-31.** `disown_job`'s no-`-h` arm now routes the dropped
row through `Shell::remember_reaped`, like every other background-row drop —
bash's `disown` deletes the job with the same `delete_job` that calls
`bgp_add`. A job disowned while still running remembers 0, which is why bash
answers 0 *immediately* rather than waiting: the shell has let go of the child
and has no status to have learnt. The `-h` arm keeps the row, so it has nothing
to remember and the job is still waited on for real. Covered by
`disowning_a_job_still_leaves_its_status_answerable_by_pid` and a new section of
`tests/corpus/jobs-disown.sh`.
