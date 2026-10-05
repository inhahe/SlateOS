### TD-OILS-THE-COMPGEN-JOB-CORPUS-CASE-IS-FLAKY-UNDER-A-FULL-SWEEP. A finished job is still offered as `running` — 2026-08-08 — ✅ FIXED 2026-08-08 (the case's margin, not osh's reaping)

**Where:** `userspace/oils/tests/corpus/compgen-job.sh`, last group ("once they
have all finished they are still completions; running is not"), against
`compgen -A running` in `userspace/oils/src/interp.rs`.

**What.** In a full `scripts/osh-bash-diff.py` sweep the case failed once with
osh listing `true` for `-A running` and exiting 0 where bash listed nothing and
exited 1 — i.e. osh still thought a job that had finished was running. Re-run on
its own it matches; the rest of the sweep (510 cases) was clean.

**Why (suspected).** Same shape as
`TD-OILS-WAIT-N-JOB-STATUS-TEST-IS-FLAKY-UNDER-PARALLEL-EXECUTION` above: the
case starts background jobs, waits for them, and then asks a question whose
answer depends on the children having been *reaped*, not merely exited. Under a
sweep the machine is loaded and that window closes late. If so the bug is in
osh's reaping — bash gets it right on the same loaded machine — and the case is
reporting a real race rather than being badly written. Do not "fix" it by
loosening the case until that is ruled out.

**Proper fix.** Establish first whether `compgen -A running` consults live
process state or a table updated only on reap, and make the job table's
transition to *done* happen where bash's does (`waitchld` / the notification
that the shell already performs before the next builtin runs).

**Impact.** An intermittently red sweep, which is the gate on every commit.

**Fixed 2026-08-08 — and the suspicion above was wrong.** It was measured
rather than reasoned about, and osh's reaping is not late. Both shells were
asked how long they *think* a two-second background job lasts (busy-polling
`compgen -A running`, which is a builtin, so the loop spawns nothing); the
overhead over the job's own 2000 ms, median of 11 runs:

| job | bash | osh |
|---|---|---|
| `sleep 2 &` | 85 ms (73–99) | **57 ms** (55–66) |
| `true \| sleep 2 &` | 100 ms (90–286) | **66 ms** (56–225) |
| `( sleep 2 ) &` | 92 ms | **67 ms** |

osh notices completion *earlier* than bash in every shape, and its extra cost
for a thread-backed job over a process-backed one (+9 ms) is smaller than
bash's for the same shape (+15 ms). So the thread-backed job's wrapper — the
tempting culprit, since the one observed failure listed `true`, the pipeline
job — is not the cause. `compgen_job_names` already calls `poll_jobs` first,
and `JobBody::is_finished` consults `try_wait` for a process and
`JoinHandle::is_finished` for a thread; neither adds a lag that bash does not
also pay.

What the same measurement *does* show is the cause: the max column. Both
shells occasionally take ~200–290 ms longer than their median, which is this
host's process-spawn latency spiking — the thing
TD-OILS-CORPUS-SWEEP-IS-UNRUNNABLE-WHEN-PROCESS-SPAWN-LATENCY-SPIKES is about.
The case settled with `sleep 1.2` against a longest job of `sleep 1.0`: a
**200 ms margin, exactly the size of a routine spike.** A spike landing on one
shell's `sleep 1.0` and not the other's is all it took, which is why it showed
up about once per sweep and never on a re-run.

The settle is now `sleep 4` (a ~3 s margin), with the reasoning in the case.
`wait` was tried first and is *not* the answer: it sweeps the job table in both
shells, so `compgen -A job` afterwards lists nothing (rc=1) and there is no
finished-but-unreaped job left to ask about — which is the whole subject of
that group. The running-job half of the case needs no such margin and did not
get one: everything between starting the jobs and the last question about
running ones is a builtin, so the shell spawns nothing there and no spike can
reach it.

Verified with 48 runs of the case at 8-way concurrency (0 failures) plus a full
523-case sweep.
