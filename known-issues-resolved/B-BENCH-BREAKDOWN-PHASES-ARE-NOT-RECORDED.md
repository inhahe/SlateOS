### B-BENCH-BREAKDOWN-PHASES-ARE-NOT-RECORDED

**Status:** FIXED 2026-08-14. **Found:** 2026-08-14, while establishing a
baseline for P21.

`bench.rs`'s `vfs_stat_breakdown_*` phases are computed, printed, and dropped.
`bench/history.jsonl` records 64 benchmarks per run and none of them are these,
so there is no longitudinal record of the phase decomposition and no way to
answer "when did namespace translation get slower?" without hand-diffing serial
logs. The comparison tooling (`scripts/bench-history.py`) therefore cannot flag
a regression in any of them.

Verified rather than inferred: `parse_serial('build/serial-test.txt')` returns
64 entries and not one of them matches `vfs_stat_breakdown_*`.

The proper fix is to record them like every other benchmark rather than to keep
reading them by eye.

**A correction to my own first draft of this entry, kept because it is the same
mistake this thread keeps recording.** I first wrote that the gap was probably
wider than these five, on the evidence that the log prints 84 `min=` lines while
the recorder writes 64 records — "20 unrecorded". That inference was worthless:
the two numbers count different things. `parse_serial` reads the **scorecard**,
which carries its own shorter names (`context_switch`, `ipc_channel`,
`vfs_throughput_16k_write`), while the `min=` lines carry the raw `run()` names
(`context_switch_rt`, `ipc_channel_roundtrip`, `vfs_write_16k`). Diffing the two
name sets gives 54 on one side and 38 on the other and means nothing at all. I
had subtracted two counts without establishing what either was counting — the
same error as deciding what the canary must be reporting instead of establishing
what it can report, committed while writing up that very error. The
breakdown-phase gap above is real and was confirmed directly; the "wider gap"
was not, and no such bug is being logged.

#### The fix, and the cause that was one level below the symptom

The symptom reads like an oversight — five benchmarks nobody remembered to
record. It was not. `score(name, result, target_ns)` was the *only* way onto the
scorecard, and it fused two independent jobs: entering a benchmark into the
longitudinal record, and grading it against a published hardware figure. A
benchmark with nothing to grade against therefore could not be recorded at all
without inventing a target, and an invented target of 0 grades as a permanent
failure and skews the pass/fail summary.

That reasoning was already written down, at the `ipc_channel_roundtrip_64k`
call site:

> Deliberately NOT added to the scorecard — a target of 0 would always register
> as a failure and skew the pass/fail summary. Report min/mean for regression
> tracking instead.

The first sentence is correct. The second is false, and is the actual bug: a
`serial_println!` is not regression tracking. That benchmark was absent from
`bench/history.jsonl` for its entire life, so the future zero-copy improvement
the comment anticipates would have had nothing to be compared against. Six
benchmarks were lost this way, not five — the five `vfs_stat_breakdown_*` phases
and this one — and any future untargeted benchmark would have been lost too,
because the API left no other option.

The fix splits the two jobs. `score()` files a measurement with a target;
`track()` files one without. A tracked benchmark is recorded in
`history.jsonl` and diffed run-over-run exactly like a scored one, and simply
never appears in the pass/fail summary or the over-target list. On the wire it
is a second `SCORE` form with `-` in the target column and `TRACK` as the
verdict:

```text
[bench] SCORE vfs_stat_breakdown_ns 263 - TRACK 310 500
```

`-` rather than `0` because "has no target" must not be confusable with "has a
target of zero and failed it" — the exact ambiguity that made the original
workaround necessary.

Three consumers had to move with it, and each would have failed silently:

* **`scripts/bench-history.py`'s `SCORE_RE`** rejects any line it does not
  match, and a rejected line is indistinguishable from a benchmark the kernel
  never measured. Widened to accept `(\d+|-)` and `TRACK`, with the target
  parsing to `None`.
* **`report_baselines`** would have listed every tracked benchmark as
  *unbaselined* on every run, growing that list until the genuinely unbaselined
  entries were lost in it. Tracked names are now counted and named separately,
  and `unused` is differenced against the *graded* names so a baseline naming a
  tracked benchmark stays reportable.
* **`/api/bench`** counted `passed`/`failed` over every entry. `target_ns` and
  `passed` are now `null` for a tracked entry rather than `0`/`false`, so a
  consumer that does not know about tracked benchmarks gets a value it must
  handle instead of a plausible-looking zero target it would render as a
  benchmark failing by an infinite margin.

`test_tracked_benchmarks_round_trip` pins all of it: the line parses, the target
is `None` and not `0`, the measurement and dispersion survive, `over_target`
does not count it, and the cross-check neither reports it as unbaselined nor
omits it from the summary.
