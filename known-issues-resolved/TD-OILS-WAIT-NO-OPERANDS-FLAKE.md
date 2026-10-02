### TD-OILS-WAIT-NO-OPERANDS-FLAKE. an intermittent `wait` status was not a race at all: a bare number resolved as a job number — ✅ RESOLVED 2026-07-29

**Where:** `userspace/oils/src/interp.rs` — `lookup_job()`, and the seven
builtins that call it.

**Was.** Filed as a *flaky test*: one `cargo test -p oils` run failed
`interp::tests::wait_without_operands_names_no_job` with `left: 3, right: 127`,
passed in isolation immediately afterwards, and did not recur. The suspicion
recorded here was a reaper/job-table race under parallel load, and the entry
said to instrument the bookkeeping if it ever came back.

**It came back — in a different harness, which is what cracked it.** A full
differential-corpus run failed `jobs-wait` on `wait-stranger`: bash `127`, osh
`0`. That case is `wait 1` on a pid that is not a child, and it sits a few lines
after a `true &`. Six re-runs in isolation passed, which looked like the same
flake again — but the probe that mattered was not a re-run. Making the job
*deterministically* still be there:

```
sleep 2 &
wait 1; echo "rc=$?"
```

diverges every single time — bash `wait: pid 1 is not a child of this shell`,
rc 127; osh rc 0, having waited on the `sleep`. So there was no race: the bug
was fully deterministic, and only its *visibility* depended on timing — whether
a job numbered 1 was still in the table when `wait 1` ran. That is exactly the
shape that masquerades as flakiness, and it is why the "suspected cause" above
was wrong.

**What.** `lookup_job()` resolved a bare (un-`%`'d) operand as "a pid first and
a job number second". bash has no such fallback, and instead splits its
builtins in two:

* `wait`, `kill`, `disown` — documented as taking "jobspec **or** pid" — read a
  bare number as a **pid** only. With `sleep 2 &` live, all three fail against
  pid 1: `wait: pid 1 is not a child of this shell` (127),
  `kill: (1) - No such process` (1), `disown: 1: no such job` (1).
* `jobs`, `fg`, `bg` — which read their operand with `get_job_spec`, where the
  `%` is optional — read a bare number as a **job number**. `jobs 1` lists it.

A `%1` reaches the job from either side. All of the above measured against
bash 5.2.

**Fix (2026-07-29).** `lookup_job` takes a `BareNumber::{Pid, JobId}` telling it
which kind the calling builtin accepts; the `%…` parsing is untouched and
shared. Call sites: `Pid` for `kill_one`, `builtin_wait`, `wait_next`,
`builtin_disown`; `JobId` for `builtin_jobs`, `builtin_fg`, `builtin_bg`.
`jobs_execute` (`jobs -x`) is `JobId` for form's sake only — it guards with
`!w.starts_with('%')`, so its bare-number path is unreachable, and bash likewise
leaves a bare `1` in `jobs -x echo 1` unsubstituted.

**Tests.** The `live-job-not-a-pid` / `kill-bare` / `kill-spec` /
`disown-bare` / `jobs-bare` / `wait-spec` block added to
`tests/corpus/jobs-wait.sh` (which pins all six shapes against bash with a job
deliberately left running, so the divergence can no longer hide behind timing),
plus `interp::tests::job_specs_name_by_number_prefix_and_substring`, which now
asserts both directions: a pid resolves only as `Pid`, a job number only as
`JobId`, and every `%` spec identically under both.

**Lesson worth keeping.** An intermittent failure is not evidence of a race. Two
observations in two different harnesses, both pointing at `wait` returning a
success where bash says 127, were enough to go looking for a *deterministic*
reproducer — and the reproducer took one try once the question became "what
state does the failing run have that the passing run does not?" rather than "how
do I make the timing worse?".
