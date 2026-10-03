### B-A-AN-ORDINARY-RUN-NEARLY-JOINED-A-LAYOUT-BAND-AS-A-SEVENTH-ARM, AND ONLY ONE FIELD STOPPED IT (lane A, 2026-08-19) -- reopened and re-closed the same day: `bench-history.py` did stop it, but an ad-hoc analysis bypassed the guard entirely and published the merged numbers. See "AMENDMENT" at the end.

**In short:** a "layout band" is a range of timings measured from six builds of
the *same* kernel placed at six different code offsets; its job is to say how
much a benchmark moves for reasons that are not a code change, so that a
movement inside that range can be dismissed. Anything that widens the band
therefore *dismisses* real regressions. An ordinary benchmark run from earlier
the same day -- not part of any sweep, and executed under hardware
virtualisation, so a median 3.5x faster -- turned out to match a genuine sweep
arm on every field that the banding code compares except one. It was rejected,
correctly, and no code needs to change. This entry exists for two reasons: the
near-miss was until now recorded only in a source comment, where nobody
reading the issue log would find it; and what the merge would have *cost* had
never been measured, only asserted. It is measured below, and it is worse than
"the band gets wider".

**Where:** `scripts/bench-history.py` -- `LAYOUT_SWEEP_TAG` (the guard that
held) and `layout_arm_rejection()` (the four checks that did not).

**The run.** `bench/history.jsonl`, `2026-08-19T16:15:09+00:00`, commit
`9011d525b`, experiment *"WHPX vs TCG: benchmark suite under hardware
virtualisation"*. Compare it field by field against the six-arm TCG sweep at
`b36a244bb`:

| field `layout_arm_rejection` consults | TCG sweep arm | the 16:15 probe | separates? |
|---|---|---|---|
| `host` | this machine | this machine | no |
| `profile` | release | release | no |
| `host_load` | unloaded | unloaded | no |
| `text_pad` | 0, 1024, 1536, 2048, 2560, 3072 | **0** -- truthfully, it was unpadded | **no** |
| source identity | `kernel` tree `6876f9eb…` | **`6876f9eb…`** -- byte-identical | **no** |
| `accel` | absent (`None`) | **absent (`None`)** -- predates the accelerator banner | **no** |
| `experiment` contains `LAYOUT_SWEEP_TAG` | yes | **no** | **yes -- the only one** |

Verified rather than argued: `git rev-parse 9011d525b:kernel` and
`git rev-parse b36a244bb:kernel` are the same object, so the digest fix of
`B-A-A-LAYOUT-SWEEP-IS-VOIDED-BY-ANY-COMMIT-MADE-WHILE-IT-RUNS` groups them
too -- it had to, since they really are the same source. The two facts that
make the probe *not* an arm are that it ran on a different accelerator and was
not asked to be one, and only the second of those is recorded anywhere.

**Every blind field is blind for a defensible reason, which is why this is not
fixable by adding a check.**

- `text_pad: 0` is *correct*. An unpadded kernel has zero padding. There is no
  value the field could hold that would mean "not a sweep arm" without lying.
- `accel: None` is correct-for-its-time: that kernel predates the banner the
  parser reads, and `None` means *unknown*, not *TCG*. Reading unknown as TCG
  is exactly the conflation `TD-A-A-BARE-METAL-RUN-RECORDS-ITS-ACCELERATOR-AS-
  ABSENT` is about, in the other direction.
- The source digest is *correct and equal*, because the source genuinely was
  equal. A digest cannot separate two runs of one binary; that is the property
  it was built to have.

So the intent -- "this run was submitted as an arm" -- is not derivable from
any measurement. It has to be *declared*, and `LAYOUT_SWEEP_TAG` is that
declaration. That is the entire argument for keeping a field whose only
content is a string a producer writes and a consumer matches, and which looks
redundant beside six fields of real data.

**What the merge would have cost, measured.** Across all 86 benchmark entries
the two runs share, the probe is a median **3.54x** faster than the median TCG
arm -- but the ratio ranges from **0.03x to 10.43x**. That spread is the part
worth noticing: a seventh arm three-and-a-half times faster on average would
not merely have widened the band by a known factor, which a reader might have
spotted as implausible. It would have *deformed* it per-benchmark, inflating
some entries tenfold, leaving others alone, and inverting a few. There is no
inspection of the resulting band that recovers "one of these came from a
different machine model" from that.

**Direction of error, again.** The tag fails safe in both directions and this
is not luck. A missing tag on a real arm splits the sweep -- expensive, loud
enough to notice (no band appears), and it dismisses nothing. A spurious tag
on a non-arm merges -- silent, and every regression inside the inflated band
is dismissed. Since the tag is written by exactly one producer
(`scripts/layout-sweep.py`, importing the constant rather than repeating the
string) the spurious direction requires someone to type the sentinel by hand
into `BENCH_EXPERIMENT`, which is as close to unreachable as a declared field
gets.

**Nothing to do in `bench-history.py`.** No code change; the guard held, and
the `LAYOUT_SWEEP_TAG` comment already names this run rather than describing a
scenario. What was missing was the entry you are reading -- the standing rule
is that an observed near-miss goes in the tracker, not only in the comment on
the code that survived it, because the tracker is what gets read when someone
is deciding whether a field is worth keeping. If a future change proposes
deriving "is this an arm?" from the data instead of the declaration, this
entry is the counterexample: on 2026-08-19 the data was identical and the
answer was no.

#### AMENDMENT (2026-08-19, same day) -- it was not a near-miss. The merge happened, outside the harness, and reached the operator's decision queue

Written a few hours after the above, and it downgrades that verdict. The
sentence "the guard held" is true of `bench-history.py` and false of the
project, because the guard is only reached by code that *calls* it.

**What happened.** The WHPX-vs-TCG comparison table in `open-questions.md`
Q53 -- the evidence for that question's option D -- was produced by a
hand-written analysis that selected and grouped arms itself instead of going
through `layout_arm_rejection()`. Its TCG column therefore included this very
16:15 probe among the TCG arms. Published figures versus the truth:

| | as published | actual TCG band |
|---|---|---|
| median | 36.5% | **26.0%** |
| mean | 104.9% | **40.6%** |
| worst | 2466% (`hpet_read`) | **182.0%** (`heap_raw_alloc_free_4096`) |
| `hpet_read` | 2466% | **6.2%** |

`hpet_read` is the cleanest possible fingerprint: it costs a VM exit under
hardware virtualisation (13,680 ns) and is emulated inline under TCG (446 ns),
so admitting one WHPX row to a TCG band inflates it ~30x. Re-adding the probe
to the TCG arms and taking a raw peak-to-peak reproduces the old table's top
three rows at the same rank and within a factor of 1.2. The lower rows are
near the clean numbers, so the published table was a mixture and cannot be
fully reconstructed -- only diagnosed.

**It failed in the predicted direction, which is the whole point.** Inflating
TCG's noise makes the case for abandoning TCG look stronger. The corrected
numbers still favour that option but by 4.8x on the median rather than 7.4x,
with the worst-benchmark collapse falling from 26x to 1.9x and the count of
benchmarks that get *worse* under WHPX rising from eight to fourteen. An
analysis that contaminates its own baseline in favour of its own hypothesis is
exactly the silent, plausible failure the direction-of-error argument in this
entry predicts -- and it was found only because the numbers were re-derived
from the raw records while updating the table for a sixth arm, not because
anything flagged them.

**So the corrected lesson is not "keep the tag".** It is that a predicate is
not a guard unless it is on the only path. `layout_arm_rejection()`'s own
docstring already says this about a *restatement* of it drifting; this is the
same failure with the restatement written in an analysis script rather than in
`layout-sweep.py`. Two consequences, both adopted:

1. **Evidence quoted in `open-questions.md` must be reproducible by a command
   the reader can re-run.** Every figure in the corrected Q53 table is printed
   by `bench-history.py --layout-bands`, which routes through
   `layout_arm_rejection` and so cannot pick up an untagged run. Recorded in
   the Q53 entry itself so it is found by the next person writing evidence
   there rather than only by someone reading this file.
2. **An ad-hoc analysis that groups records must call the real predicate.**
   `build/whpx-band-preview.py` did -- it monkeypatches the *key* and reuses
   `layout_arm_rejection` unmodified, which is why its WHPX column is correct
   and survived the re-derivation intact. That is the pattern to copy; the TCG
   column was produced by something that did not, and did not survive.

The original verdict above is left as written rather than edited, because
"CLOSED, the guard held" being wrong within hours of being written is itself
the useful record.
