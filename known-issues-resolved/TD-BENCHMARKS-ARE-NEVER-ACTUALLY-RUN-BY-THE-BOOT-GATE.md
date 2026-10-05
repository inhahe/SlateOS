### [A] TD-BENCHMARKS-ARE-NEVER-ACTUALLY-RUN-BY-THE-BOOT-GATE. The whole performance suite — baselines, targets, scorecard — is spawned and then killed mid-run on every boot test — 2026-08-14 — **FIXED** (all three options landed; see "Closed 2026-08-14" below)

**Where:** `kernel/src/main.rs` (`deferred_bench_task`, spawn site ~5505),
`kernel/src/bench.rs` (`run_all`, `score`, `SCORECARD`),
`scripts/boot-test.sh` (`WAIT_MARKER`, default `BOOT_OK`),
`bench/baselines.toml`.

**The shape of it.** Benchmarks run in a deferred low-priority kernel task that
prints `BENCH_OK` *after* `BOOT_OK`. That deferral is itself correct and well
reasoned — the comment explains it gets init to a prompt in ~1 s instead of
~20 s under TCG. The problem is the other half: the routine boot test waits for
`BOOT_OK` and tears QEMU down at once, so the bench task is killed before it
produces numbers.

**Evidence.** In the clean 26094-line KASAN boot
(`build/serial-kasan-pass.txt`), `[bench] === Kernel micro-benchmarks ===` is
line 26092 — the **second-to-last line in the file**. The task got just far
enough to print its own header before QEMU died. In an ordinary boot log the
header does not appear at all. Neither log contains a single benchmark result
or a `BENCH_OK`.

**Why it matters.** This is the reason
`B-FAST-CPU-INDEX-FELL-BACK-TO-AN-APIC-MMIO-READ-ON-EVERY-ALLOC` shipped
unnoticed: CLAUDE.md requires benchmarking after any change to a
performance-critical subsystem, `page_alloc_free` has a recorded QEMU baseline
of 198 ns / 736 cycles to compare against, `score()` computes a pass/fail
verdict — and none of that machinery has executed in the harness. A suite that
is never run is worse than no suite, because its existence is taken as
coverage.

**Same class as `B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT`** (above): a check that
silently did not run while the boot reported PASSED. That one was fixed by
making the skip *loud*. The same principle applies here.

**Proper fix.** `scripts/boot-test.sh --bench` already exists and does the right
thing — it switches `WAIT_MARKER` to `BENCH_OK` and surfaces `ABOVE TARGET`
verdicts — it simply is not part of any routine gate. Options, in preference
order:

1. Make the *absence* of benchmark results loud rather than silent, mirroring
   the Path-Z fix: have the boot test note when it terminated with the bench
   task still pending, so "no numbers" is visible instead of assumed-fine.
2. Run `--bench` on a schedule rather than every boot (it roughly doubles the
   ~405 s cycle under TCG, which is why making it the unconditional default is
   unattractive), specifically after any change touching `mm/`, `sched/` or
   `ipc/`.
3. Record the scorecard to a file the harness can diff across runs, so a
   regression is a *comparison* rather than a threshold — thresholds as loose
   as these (1000 ns against a 198 ns baseline) would not have caught a 3-4x
   allocator regression anyway.

Note that (3) is the one that would actually have caught the bug that motivated
this entry: a 736 → ~2500 cycle regression still passes a 3700-cycle target.
The targets are sized against Linux, not against our own last-known-good.

**Progress 2026-08-14 — (3) is DONE; (1) and (2) remain open.**
*(Superseded later the same day: (1) and (2) are now done too — see
"Closed 2026-08-14" at the end of this entry.)*

`print_scorecard` now emits a machine-readable line for **every** entry, not
just the failures:

```text
[bench] SCORE <name> <measured_ns> <target_ns> <PASS|OVER>
```

`scripts/bench-history.py` parses those out of the serial log, appends a
JSON-lines record (timestamp, host, git commit, all measurements) to
`bench/history.jsonl`, and diffs the run against the previous record **from the
same host**, reporting anything that moved more than a threshold (default 25%)
plus benchmarks that appeared or vanished. `boot-test.sh::print_bench_results`
invokes it automatically, non-fatally.

Three things about the design are deliberate:

* **Passing entries are recorded too.** The old failure-only list was blind to
  precisely the bug that motivated this entry — a benchmark that doubles while
  still beating a Linux-sized target never appeared in the output at all.
* **Diffs are same-host only.** A different machine or QEMU build moves every
  number at once; reporting "no baseline" beats reporting a hardware difference
  as a regression.
* **Over-target is no longer phrased as a failure**, in the kernel output or in
  `boot-test.sh`. It is labelled reference. That follows directly from
  `TD-BENCH-OWNER-AB-BUDGET-WAS-AN-ABSOLUTE-CYCLE-COUNT`: under TCG the
  hardware targets are unreachable by construction, so treating them as
  verdicts trains the reader to ignore the whole suite — which is how a real
  regression hid in it.

`boot-test.sh` previously advised "compare against prior runs rather than
treating this as a hard regression" while nothing stored prior runs, making the
advice unfollowable. It is now followable.

Still open: (1) making the *absence* of benchmark results loud on a routine
non-`--bench` boot, and (2) deciding when `--bench` runs, since it roughly
doubles the boot cycle under TCG.

**Closed 2026-08-14 — (1) and (2) landed together, because (2) turned out to
be answerable by (1) rather than by a schedule.**

(1) is `report_bench_absence()` in `scripts/boot-test.sh`, called on both PASS
paths whenever `--bench` was *not* given. It prints a `=== NO BENCHMARK
RESULTS THIS RUN ===` block and never changes the exit code — a routine boot
is *allowed* to skip the suite. The point is only that `PASSED` must not be
readable as "performance was checked". It distinguishes the two states the log
can be in: the deferred task started and was killed at `BOOT_OK`, or it never
reached its first result.

(2) as written — "run `--bench` on a schedule … after any change touching
`mm/`, `sched/` or `ipc/`" — assumes a scheduler that does not exist here, and
assumes someone remembers the rule at the right moment. That is the same
failure mode as the original bug: coverage that depends on being remembered.
Since `bench-history.py` already stamps every recorded run with its git
commit, the harness can just *compute* the answer instead:

```sh
git diff --name-only <last_benchmarked_commit> HEAD -- kernel/src/{mm,sched,ipc,syscall} kernel/src/smp.rs
```

Non-empty ⇒ this boot contains unbenchmarked changes to code CLAUDE.md
requires benchmarking, and the block escalates to `!! Performance-critical
code changed since the last benchmarked commit`, naming the files. Empty ⇒ it
says skipping the suite is reasonable here. So the nag is targeted and
automatic rather than periodic, and it cannot be forgotten.

Degenerate cases are handled explicitly rather than by silence, since the
whole entry is about silence: no `history.jsonl` yet ⇒ "no baseline for this
host"; a recorded commit absent from the repo (rebased away, or not fetched)
⇒ say so rather than diffing against nothing and reporting a false all-clear.

**Verified** by exercising all six branches against real and synthetic serial
logs: suite-started-then-killed, suite-never-started, perf-critical files
changed, nothing changed, missing history, unknown commit. On the current tree
it correctly reports `kernel/src/syscall/{handlers,number}.rs` as changed
since `bf26aabdb`.

**Not fixed by this, and deliberately so:** the kernel still spawns the
deferred bench task on every boot and still has it killed at `BOOT_OK`. That
wasted work is cheap (the task prints a header and dies), and suppressing the
spawn on non-`--bench` boots would need a kernel cmdline flag for no real
gain. The defect was never the wasted work — it was that nobody could tell it
had happened.
