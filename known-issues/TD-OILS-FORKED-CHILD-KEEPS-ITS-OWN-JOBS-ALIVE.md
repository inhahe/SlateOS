### TD-OILS-FORKED-CHILD-KEEPS-ITS-OWN-JOBS-ALIVE. bash's `mark_all_jobs_as_dead` also writes off the child's *own* running jobs; osh tells the truth — OPEN (deliberate) — 2026-08-03

**Where:** `userspace/oils/src/interp.rs` — `Shell::mark_inherited_dead`, which
is restricted to rows marked `inherited`.

**Reproduce:**

```sh
sleep 30 >/dev/null 2>&1 &
echo "$( sleep 30 >/dev/null 2>&1 & wait %1 >/dev/null; jobs )"
```

bash lists *both* rows `Done` — including job 2, which the substitution started
itself moments earlier and which is plainly still running. osh lists the
inherited row `Done` and its own row `Running`.

**Why not emulated.** bash's `mark_all_jobs_as_dead` is exactly what its name
says: it walks the whole table on `ECHILD` rather than the rows the `ECHILD` was
about. For an inherited row that is right by accident — the child really will
never hear the end of it — but for a job of the child's own it is a lie the
child can immediately disprove, and a later `wait` on that job then blocks and
answers correctly anyway. Copying it would mean deliberately misreporting live
state.

**Impact.** A fork that starts a job of its own, waits on an inherited one, and
then lists. The corpus case exercises the inherited half and stays clear of this.
