### TD-OILS-WAIT-N-JOB-STATUS-TEST-IS-FLAKY-UNDER-PARALLEL-EXECUTION. `wait_n_ignores_a_job_whose_status_was_already_reported` fails about one run in three — 2026-08-08 — FIXED 2026-09-14

**Where:** the `jobs`-listing assertion in
`interp::tests::wait_n_ignores_a_job_whose_status_was_already_reported`
(`userspace/oils/src/interp.rs`, `assert_eq!(listing(&mut sh,
"jobs").lines().count(), 1)`).

**What.** Under the full `cargo test -p oils --lib` run the assertion sometimes
reports `left: 0, right: 1`; the same test passes every time when run alone
(`cargo test … wait_n_ignores`). Two full-suite runs in a row: one failure, then
one clean. Nothing else in the suite failed either time.

**Why — traced 2026-08-08, and neither of the two originally suspected.** The
count that comes back 0 is a `jobs` listing, so what varies is not a status but
whether the job's *row is still there*. The chain that keeps it is exact, and
every link is in `interp.rs`:

1. `( exit 5 ) & p=$!; sleep 0.2` starts a thread-backed job. Nothing polls the
   table during a command — every `poll_jobs` call site is inside a job builtin
   (`jobs`, `wait`, `kill`, `disown`, `fg`, `bg`, `compgen`) — so the only poll
   before the `wait` is the read-parse-execute loop's own
   `cleanup_dead_jobs` at the unit boundary (`run_source_flow_result`, the
   `if !self.in_comsub` sweep).
2. That poll is what sets `exit_seen`, and it sets it only for a body that is
   *already* finished (`JobBody::is_finished`), plus `born_at.elapsed() >=
   JOB_EXIT_NOTICE_GRACE` (20 ms).
3. `drain_jobs` snapshots `known` — the `exit_seen` jobs — **before** its own
   `poll_jobs`, deliberately, so that the `wait`'s instantaneous glance does not
   count as prior knowledge. A job in `known` is skipped and stays unnotified.
4. A job *not* in `known` is waited for, marked `notified`, and the next unit
   boundary's `cleanup_dead_jobs` retains only `status.is_none() || !notified` —
   so the row is swept and the following `jobs` prints nothing.

So the test would fail if the `( exit 5 )` thread had not finished within the
200 ms `sleep 0.2`. That is the only route to a 0 here — but **measurement says
it is not the route taken**, and the trace above is recorded as the mechanism
ruled *out*, not as the diagnosis.

**Measured 2026-08-08, after the trace — and it does not reproduce.** Three
probes, all with the suite's own contention:

- Thread completion for `( exit 5 ) &`, worst of 25 under a full-suite run:
  **2.3 ms** to spawn, **10.5 ms** to finish. The race it is supposed to lose is
  against 200 ms — a 20× margin.
- The test's exact sequence, sampled 8× under a full-suite run: `sleep` works
  (`rc=0`), the unit takes ~234 ms, and the job comes out `exit_seen: true,
  status: Some(5), notified: false` with its row kept — every time.
- **54 full-suite runs with no occurrence:** 14 sequential on a quiet machine,
  then 40 more as four concurrent copies of the test binary.

**What this entry is now.** The frequency claim ("about one run in three") is
unreproducible as written, and the `interp.rs:78623` in the original **Where**
no longer points at this test at all — the file has grown past it, which is what
sent the first pass down the wrong chain. A likelier reading is that the
original observation belonged to the scratch-name collision class rather than to
jobs: `uniq_name`'s own doc records a previous **~1-in-3** flake of exactly that
frequency (`TD-OILS-TEST-SCRATCH-NAME-COLLISION`), striking "arbitrary tests"
with unrepeatable failures — and the last six fixed-name scratch files were only
removed in the preceding commit (`3badf52b3`).

**Proper fix.** Do not change the test on this evidence; there is nothing
measured to fix. Leave it OPEN as a watch item, and if it is ever seen again
capture the *whole* failure — test name, assertion, `left`/`right` — before
theorising, since the stale line number is what made the first attempt
misidentify the assertion.

**Impact.** A red full-suite run that is not a real regression, which is the
worst kind: it teaches the next session to re-run and shrug.

**Note added 2026-08-08.** TD-OILS-THE-COMPGEN-JOB-CORPUS-CASE-IS-FLAKY-UNDER-A-FULL-SWEEP
called itself "the same shape as" this entry, and it has now been diagnosed —
but as something else, so **do not carry that link forward**. That one was this
host's *process-spawn* latency spiking past a 200 ms margin. This test has no
process in its critical path: the job is thread-backed, and the only spawn
(`sleep 0.2`) is the thing being waited *for*, so a spike there lengthens the
margin instead of eating it. The two share only the symptom "a job's state is
read too early". Measuring the reaping is, however, worth repeating here if it
recurs: for the compgen case that measurement is what refuted the reaping
theory outright (osh notices completion 20–30 ms *earlier* than bash in every
job shape), and the same is likely true here.

**Sighting 2026-08-14, and the detail was lost — read this before the next
one.** One full-suite run came back `1490 passed; 1 failed` while a full
`osh-bash-diff.py` sweep was running alongside it, i.e. with several hundred
`osh`/`bash` processes being spawned on the same host. Three re-runs
immediately after, still under the same sweep, were clean (`1491 passed`). The
failing test's **name was not captured**, so this cannot be attributed to this
entry or to any other — and that is the avoidable part: the run went through
`run-timeout.py … | tail -5`, which keeps the summary line and throws away the
`failures:` block above it, and a second attempt to re-read it with `grep` hit
`Binary file (standard input) matches` because a corpus test writes NUL bytes
to stdout. **Redirect to a file** (`cargo test … > /tmp/lt.txt 2>&1`) and grep
the file, rather than piping, so a one-in-many failure is not thrown away the
moment it finally happens.

**Follow-up 2026-08-14.** The advice above was taken and it worked: a corpus
case flaked under a sweep later the same day and the whole divergence *was*
captured, so it is written up on its own evidence rather than guessed at — see
TD-OILS-THE-WAIT-NO-OPERANDS-CORPUS-CASE-IS-FLAKY-UNDER-A-FULL-SWEEP. That one
is a **different** fault from this entry (a corpus case's `wait -n`, not this
lib test's `jobs` listing) and, unlike the compgen case, is *not* a thin margin
— so do not merge the three. What they share is only the discipline: keep the
saved `corpus-failures/` report, and record which loads failed to reproduce.

**FIXED 2026-09-14 in `bf4a55387`, and it was not a test defect.** This entry
spent five weeks filed as a flaky *test*; the chain traced on 2026-08-08 above
is correct in every link and stops one step short of the fault.

`poll_jobs` set `exit_seen` INSIDE the branch guarded on `child` still being
`Some` — and that branch's own first act is to take `child`. So a poll landing
between the body finishing and `JOB_EXIT_NOTICE_GRACE` (20 ms) elapsing reaped
the job with `exit_seen` left false, and no later poll could ever set it,
because the guard it needs can never be true again. `exit_seen` was a property
of WHICH POLL happened to observe the exit rather than a property of the job.

The empty listing follows: `drain_jobs` reads `exit_seen` as "did the shell
already know?", so a job with it false counts as one this `wait` waited FOR and
is marked notified — before `builtin_wait`'s pass that spares `$!`, which only
ever sets `notified = true` and so cannot rescue it. The next `jobs` sweeps it
and prints nothing.

**Load was never the variable.** It only bought more attempts at a 20 ms
window: 0 failures in 250 isolated runs, 0 in a full `oils` suite, 0 in three
shuffled orders, and 1 in 25 with a dozen other test binaries competing. That
is why "passes alone, fails under `--workspace`" read as a test-harness problem
for five weeks. A product bug that needs a 20 ms window looks exactly like a
flaky test, and the way out was to force the window rather than to keep
sampling it — `a_poll_before_the_grace_does_not_lose_the_exit_forever` does
that and fails 100% without the fix.

Reported twice by lane C, whose second report added the detail that made it
worth re-opening rather than re-running.
