## A-EVERY-HISTORY-ROW-WRITTEN-BEFORE-2026-08-18-MAY-NAME-A-COMMIT-IT-DID-NOT-MEASURE (lane A, 2026-08-18) - **fixed forward in `7eec5d294`; the old rows are still wrong**

**In short:** `bench/history.jsonl` and `bench/boot-history.jsonl` each stamp a
row with a git commit. Until `7eec5d294` that commit was read when the run
*finished*, not when the kernel was *built* - and a boot test runs for ten to
twenty minutes, during which committing is normal and encouraged here. So any
row whose run overlapped a commit names the wrong code. The fix stops it
happening again; it cannot repair the rows already written.

**How it was found.** A boot test validating the `kernel_main` split was started
at 02:35 against `3cb8ebcf3`. A known-issues commit was made at 02:47 while QEMU
was still running. The row the run appended reads `"commit": "88e93fecf"` - the
markdown commit. The kernel that passed contained none of it.

**Why it is worse than a mislabel.** `report_bench_absence()` in
`scripts/boot-test.sh` diffs `HEAD` against the last recorded commit to decide
whether performance-critical code has changed since anything was last
benchmarked. A row stamped *newer* than the tree it measured makes that diff
come back empty, so the harness reports "no perf-critical changes since the last
benchmarked commit" about changes that were never benchmarked. It fails in the
reassuring direction and prints a specific-looking hash while doing it.

There is a second, quieter version: nothing recorded whether the tree was dirty,
so a row for commit X measured "X plus uncommitted work" and was indistinguishable
from a row that measured X.

**What is fixed.** `boot-test.sh` captures branch/HEAD/dirty once, before the
build - it already did, for the banner, and then discarded them - and passes
them to both recorders as `--commit` / `--branch` / `--dirty`. Rows gain a
`dirty` boolean. `report_bench_absence()` qualifies its verdict when the
baseline row was dirty rather than implying a precision it does not have. Both
recorders still fall back to asking git when invoked standalone.

**What is NOT fixed, and cannot be.** Every row written before `7eec5d294`:

* carries no `dirty` field, so it is unknown whether it measured its commit or
  its commit plus uncommitted work. Readers should treat the absence as
  "unknown", not as "clean" - the same rule `bench-history.py` already applies
  to the absent `profile` field on pre-2026-08-14 records.
* may name a commit made during its own run. There is no way to detect which:
  the row does not record when the *build* happened, only the run's end
  timestamp. Rows whose `commit` is an unusually small, documentation-only
  change are the suspicious shape.

Practical consequence: **do not bisect on a pre-2026-08-18 row's commit without
checking that the commit plausibly touched kernel code.** For longitudinal
questions ("is this benchmark drifting?") the rows are still fine; it is only
the attribution of a number to a *specific commit* that is unreliable.
