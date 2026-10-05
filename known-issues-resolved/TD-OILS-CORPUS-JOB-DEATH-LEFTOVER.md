### TD-OILS-CORPUS-JOB-DEATH-LEFTOVER. `job-death-notice.sh` was a flaky case: an unreaped TERM'd job made every later section's job numbering a race — ✅ RESOLVED 2026-07-29

**Where:** `userspace/oils/tests/corpus/job-death-notice.sh`, the
`…and its number is free for the next job` section and the two blocks after it.

**What.** The case failed roughly once per full corpus run and passed every time
in isolation — the same shape as TD-OILS-WAIT-NO-OPERANDS-FLAKE, and again *not*
a race in osh. The section ended with a bare `kill %1`, and TERM is one of the
signals the shell deliberately does not announce, so nothing forced it to notice
that death: the row stayed in the job table for an unbounded time. The two
sections after it then named jobs by number (`kill -TERM %2`, `kill -PIPE %1`)
relative to a table that either still held that row or did not, and the mismatch
cascaded — with the row present, bash numbered the following jobs 3/4/5 where osh
used 1/2/3, and the final `kill %1 %3` reported `no such job`.

The divergence is *bash's* to vary, not osh's: osh notices a kill it performed
immediately (TD-OILS-JOB-DEATH-ANNOUNCE-LAG, above), so it always reported
`Terminated`, while bash needed to get round to reaping. Under no load bash
reported `Terminated` 20/20 times, which is why the case looked stable; with six
busy-loop shells running it reported `Running` on 3/10 runs. That reproducer is
what turned "flake" into a diagnosis.

**Fixed.** `wait %1` after the `kill %1`, so the job is definitely reaped before
anything else looks; and the two following blocks now run in `( … )` subshells and
create *both* of the jobs they name, so each starts from an empty table and leaves
nothing behind. Verified 8/8 under the same load that broke it 3/10.

**Lesson worth keeping.** The file's header already required a settle delay after
every kill, and every *inspection* had one. What it did not cover is a kill whose
consequences are inspected by a *later section* — the leftover row is the state
that differs between a passing and a failing run. When a job-control case is
flaky, look for what an earlier section left in the table, not for a race in the
section that failed.
