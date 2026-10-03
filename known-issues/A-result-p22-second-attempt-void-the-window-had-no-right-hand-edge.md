### [A] RESULT P22, second attempt — VOID: the window had no right-hand edge — 2026-08-18

**In short:** the experiment was run again, with the stimulus-measuring gate
that the first attempt's failure produced. It is void as well, for a different
and more embarrassing reason: the benchmark name given as the *end* of the load
window does not exist as far as the load controller is concerned, so the load
switched on where it was told and then never switched off. The label said
"positions 61-73". What actually happened was "position 61 to the end of the
boot". Nothing about the drift model can be concluded from it, and — this is
the point of writing it down — the benchmark numbers looked *good*.

#### What happened

The run was launched as:

    ./scripts/canary-load-test.sh --bench         --load-at=crypto_sha256_1KiB --load-until=vfs_throughput_16k_write

`vfs_throughput_16k_write` is a real benchmark. It is on the scorecard, it is
in `bench/history.jsonl`, and `grade-positional.py` places a window with it
without complaint. It is also a name the load controller can never see.

The kernel prints two different things under two different names:

| | printed when | name used | who reads it |
|---|---|---|---|
| live result line | as each benchmark finishes | `vfs_write_16k` | `canary-load.py`, live, to time the load |
| `SCORE` line | after the whole suite | `vfs_throughput_16k_write` | `bench-history.py`, `grade-positional.py` |

They agree for 59 of the 86 scored benchmarks and disagree for 27. `--at` and
`--until` match the *live* line, because that is the only one that exists while
the suite is still running. A human choosing a window reads the *scorecard*,
because that is what the positions are numbered against. So the natural way to
pick a window bound produces a name that cannot work, and nothing said so.

The controller waited for a `vfs_throughput_16k_write` line that was never going
to come, held the load until the harness stopped it, and exited **0**. Its
record said `outcome: stopped`, `released: false` — the fault was written down
and nothing looked at it.

#### Why this run must be discarded even though it looks good

Graded against the label, the model did well:

- 13 of 13 benchmarks in the labelled window were flagged;
- 27 of 30 flags were within reach of the labelled window;
- 3 false positives, and they are adjacent (positions 49/50/51).

The measured-stimulus gate refused it anyway, and was right to: the window's
best-case ratio was ×1.07 against a 1.15 threshold. But the *reason* it printed
— "the stimulus never reached the window" — was a true sentence pointing at the
wrong fault. A reader would have gone looking for a load that was too weak or
mistimed. The load was neither. It was unbounded.

That is the general lesson, and it is the same one the first attempt taught in a
different costume: **a run that cannot answer the question must say so for the
right reason, or the next person debugs the wrong thing.**

#### The part that is still unexplained

The load ran to the end of the boot. Positions 81-85 should therefore have been
the *most* disturbed benchmarks in the run. They measured clean — best-case
×1.09, ×1.02, ×1.03, ×1.00, ×0.72 — and the in-kernel canary read 5.16 at
sample 80, its undisturbed value. The spinners were checked and did not
self-terminate (their deadline is `started + 1800 + 120`).

So either the load stopped being effective before the suite ended, or the
benchmarks at the end of the suite are insensitive to host CPU load. Both are
possible and they have opposite implications for every future run. The serial
log carries no timestamps, so nothing recorded in this run can distinguish them.

This is the specific question the **host-side canary** added afterwards exists
to answer: a thread on the host re-times a fixed unit of work every 50ms and
timestamps each sample, so the next run can say when the machine actually became
busy and when it stopped being busy, independently of anything the kernel
reports. Until that has run, treat "the load was applied" and "the machine was
slow" as two separate claims with no evidence connecting them.

#### One more thing this run showed: the `mean` is not a usable per-benchmark statistic

On the *undisturbed* prefix of this run, per-benchmark mean ratios ranged from
×0.06 to ×34.10, while best-case ratios over the same benchmarks stayed within
×0.9-×1.1. A single preemption inside one iteration of a 2000-iteration
benchmark moves the mean by an order of magnitude; the best case only moves when
every iteration was slowed.

This is why the stimulus gate's two statistics are not equal partners, and
§230 has been corrected to say so: the best-case ratio is the discriminating
one and the mean is a weaker corroborating condition. Do not read a
per-benchmark mean ratio as evidence of anything on its own.

#### What was changed as a result

1. **The kernel now states the pairing.** `[bench] MEASURED-AS <scored> <live>`,
   printed for each benchmark whose two names differ. The kernel is the only
   party that knows it — `BenchResult::seq` already indexes the measurement
   window exactly — and neither host-side reconstruction is sound. Aligning the
   two orders fails because they genuinely interleave (`lock_uncontended` is
   recorded *after* `lock_tracked_nested` but measured *before* it); it mapped
   `io_ring_nop` onto `page_fault_anonymous` and left `page_fault` with no
   candidate at all. Parsing `bench.rs` recovers 21 of 27 and cannot recover the
   six that build their `BenchResult` by hand — `isr_latency`'s struct says
   "isr_latency" and the line it prints says "isr_hard_irq".
2. **A scorecard name is now translated, and the translation is announced.**
   Silence there would be the same fault in a nicer suit.
3. **Both directions are refused before the boot.** `--known-names` rejects a
   name that is not a live line; `--require-scored` rejects one the grader
   could not place a window with. A bad name costs a second instead of a
   two-and-a-half-minute boot.
4. **The controller exits non-zero when a trigger or release never matched**,
   and records `problem` saying which.
5. **The grader accepts that confession** and grades UNGRADED on it *ahead of*
   the measured-stimulus complaint, so the printed reason names the real fault.
   It derives the same conclusion from the record's own booleans when the field
   is absent, so it cannot be talked out of a verdict by an older or quieter
   controller.
6. **A host-side canary** now measures whether the host actually got busy.

#### Status

- **P22(a): still UNGRADED.** Two attempts, two void runs, no result.
  *(Superseded 2026-08-19 — the third attempt graded SUPPORTED; see "RESULT P22,
  third attempt" immediately below. This section is left exactly as written: it
  records what was known at the time, and the two void runs are the reason the
  third one could be trusted.)*
- The model's apparent performance on this run (13/13 sensitivity, 3 false
  positives) is **not** evidence and must not be cited as such. It was measured
  against a window that is not the window that was applied.
