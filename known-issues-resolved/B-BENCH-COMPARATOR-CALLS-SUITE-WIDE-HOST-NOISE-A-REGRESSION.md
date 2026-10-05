### [A] B-BENCH-COMPARATOR-CALLS-SUITE-WIDE-HOST-NOISE-A-REGRESSION. The run-over-run diff named six regressions in code that had not changed — FIXED 2026-08-14

**Where:** `scripts/bench-history.py`, `diff()` / `report()`.

**Symptom.** The first post-merge `--bench` run (commit `17dbde179`, host
`Logoplex3`, BOOT_OK, exit 0) reported:

```
  REGRESSED (>25% slower):
    firewall_check: 270ns -> 482ns (+79%)
    shm_create_close: 58556ns -> 84996ns (+45%)
    ipc_semaphore: 11676ns -> 16112ns (+38%)
    net_veth_roundtrip: 47097ns -> 60102ns (+28%)
    net_veth_send: 23240ns -> 29552ns (+27%)
    io_ring_nop: 1948ns -> 2460ns (+26%)
```

**Why it was wrong.** `git diff bf26aabdb 17dbde179` over the perf-critical
paths is **two files, +54/-8**: `kernel/src/syscall/number.rs` (doc comments)
and `kernel/src/syscall/handlers.rs` (`sys_thread_join` moving its exit value
to an out-pointer). Nothing under firewall, veth, shm, semaphore or io_uring
changed at all, so not one of the six flagged benchmarks executes a line that
differs between the two commits.

The actual distribution over all 63 benchmarks: **median +6.1%, mean +9.4%,
48 slower vs. 15 faster** — and the sorted tail is a smooth continuum,
`24.4, 24.5, 24.6, 24.9, 26.3, 27.2, 27.6`. There is no gap anywhere near the
threshold. A real regression is a few outliers standing clear of a ~0% median;
what this was is a fixed 25% line drawn through the middle of a shifted
distribution.

**Root cause.** The module docstring claims run-over-run comparison "cancels
the emulation constant". That holds across *hosts*, not across *runs on one
host*: TCG is pure emulation and therefore CPU-bound, so whatever else the
machine was doing scales the whole suite by a common factor. Shift a
distribution whose own per-benchmark wobble already reaches ~20% by a further
6% and its tail crosses 25%. The `diff()` docstring even anticipated the noise
("a 10-20% wobble carries no information") but chose the wrong remedy — a
coarser *absolute* threshold cannot subtract a *global* shift, it can only
trade false positives for false negatives.

**Fix.** Added `global_drift()`: the **median** of every benchmark's
run-over-run ratio, used to normalise each ratio before thresholding, so the
threshold applies to how a benchmark moved *relative to its peers on the same
run*. The median (not the mean) is the estimator precisely because it is
unaffected by a genuine regression in a minority of benchmarks — the signal
that must not be subtracted away. Skipped below `MIN_SAMPLES_FOR_DRIFT = 8`,
where the median means nothing and a handful of benchmarks can legitimately
all move together. The report now prints the drift itself (information in its
own right — it says the machine was busy), shouts if it exceeds 15%, and shows
both numbers per entry (`+68% vs suite, +79% raw`) so no one has to trust the
correction blindly.

Replayed against the real data, the four pure-drift entries drop out and the
report goes from six regressions to three.

**Why this mattered enough to fix immediately.** It is the same class of defect
as the bug that produced this harness in the first place
(`TD-BENCHMARKS-ARE-NEVER-ACTUALLY-RUN-BY-THE-BOOT-GATE`): a report you cannot
act on. A silent skip trains you not to notice; a comparator that cries wolf on
every run trains you to skim past the one time it is right. Six false
regressions on the *very first* run it was used in anger would have retired the
feature within a week.

**Related precedent:** `TD-BENCH-OWNER-AB-BUDGET-WAS-AN-ABSOLUTE-CYCLE-COUNT`
burned five boots on "ownership tagging costs 8500 cycles" that was also the
emulator rather than the code. Same underlying trap, one level up.
