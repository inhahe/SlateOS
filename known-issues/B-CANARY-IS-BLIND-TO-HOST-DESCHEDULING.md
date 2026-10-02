### B-CANARY-IS-BLIND-TO-HOST-DESCHEDULING — it read 0% (its cleanest possible verdict) on the most contaminated run yet measured

**Status:** OPEN, found 2026-08-15 by the repeat boot run for P21. **Severity:
high** — this is the run-level gate that decides whether *any* benchmark result
is admissible, and it does not fire on the dominant contamination mode.

#### The observation

Two boots of the **identical binary**, launched the same way, minutes apart:

| | run 2 (`f79aec561`) | run 3 (`c893184fa`) |
|---|---|---|
| wall time | 160 s | **365 s (2.3×)** |
| benchmarks flagged REGRESSED | 4 | **9** |
| `isr_latency` | 24 360 ns | **49 999 ns (2.05×)** |
| `page_fault` | 2 215 ns | **4 292 ns (1.94×)** |
| dispersion stalls (mean/min ≥ 5×) | 3 | **8** |
| **canary spread** | 12 % | **0 %** |

Run 3 took more than twice as long, doubled two unrelated kernel benchmarks, and
scattered regressions across nine subsystems with no common mechanism. The
canary — the instrument whose entire job is to answer *"was this run
disturbed?"* — reported **0 % spread over 11 samples**, which is the cleanest
reading it is capable of emitting. It is not merely insensitive here; on this
pair it is **anti-correlated** with the disturbance.

The dispersion detector, by contrast, tracked it correctly: 3 → 8 stalls.

#### Why it is structurally blind (the mechanism, not a guess)

The canary measures the cost of a **reference memory access** in **guest
cycles**, sampled between benchmarks. Host descheduling of the QEMU process
cannot inflate that number: TCG executes the sampling loop as a translated
block, and the guest's cycle counter advances by the *emulated* amount
regardless of how long the host took to get round to executing it. The wall-clock
time the host stole lands **between** guest instructions, where a short
guest-cycle measurement cannot see it.

So the canary can only detect something that changes the cost of the reference
access *within the guest* — cache pressure from the guest's own workload, say.
It is constitutionally incapable of detecting the host stealing the CPU, which
on this machine is the *dominant* contamination mode and the exact scenario
`boot-test.sh`'s own header warns about at length.

Benchmarks with a longer span do see it, as a wall-clock excursion inflating
their **mean** while their **min** survives — which is precisely what the
mean/min dispersion ratio measures. That is why dispersion moved and the canary
did not.

#### What this invalidates

**P19's conclusion needs a one-sided reading.** The `--bench` header says *"The
canary reports it as CONTAMINATED — believe it."* That remains true in the
direction it was demonstrated: when the canary **fires**, it is right. The
converse — which is how it has been used ever since, including by me earlier
today when I called the P21 run "canary clean, 12 % spread" as though that
certified the run — does not follow. **A clean canary is not evidence of an
undisturbed run.** Every "canary OK" in this file's history should be read as
"the canary found nothing", not "nothing was there".

It also explains `B-CANARY-TOLERANCE-CANNOT-BE-FITTED-THE-CONTROLS-ARE-DISCARDED`
from the other end: the tolerance could not be fitted from the controls because
the quantity being thresholded largely does not respond to the contamination the
threshold is meant to catch. Tuning `CANARY_TOLERANCE_PCT` was never going to
work.

#### Proper fix

The run-level verdict must stop resting on the canary alone. Three signals
exist, and two of them are already computed and thrown away:

1. **Dispersion count** — already computed and printed; not part of any verdict.
   It responded correctly here (3 → 8). Promote it: a run with a materially
   elevated stall count is CONTAMINATED regardless of the canary.
2. **Wall time** — the single most sensitive signal in this whole episode
   (160 s vs 365 s, unmissable) and it is not recorded in `history.jsonl` at
   all. Record it per run; it is free.
3. **Host load at measurement time** — still unrecorded, which is why run 3's
   cause cannot be established retroactively. This is the `--host-load` work
   already queued.

Then the canary keeps its job as a *positive* detector (it fires → contaminated)
and loses its implied role as a *negative* certificate.

#### What is still unexplained, and must not be quietly attributed

`net_ipv6_parse` reads **80 → 113 → 169 ns** across the pre-Cow baseline and the
two post-Cow runs. Both post-refactor readings exceed the 16-run historical
maximum of 96 ns, so unlike `page_alloc_free` (752 → 503, back in range) and
`http_percent_decode` (785 → 536, back in range) it did **not** revert. That is
suggestive of a real effect — plausibly code layout, since the refactor shifts
addresses kernel-wide — but run 3 is now known to be contaminated, so it cannot
carry the weight of the second data point. **Left explicitly unattributed**
pending a re-measurement on a host whose load is actually recorded. It is
logged here rather than in the P21 entry so that a −79 % headline does not end
up sitting on top of an unexamined +2× on an unrelated benchmark.
#### FIXED 2026-08-15 (instrument), with one correction to the prescription above

The run-level verdict no longer rests on the canary. `scripts/bench-history.py`
now computes a **three-axis** verdict — canary, dispersion, wall clock — and
takes the **worst** of them, so any one axis can condemn a run and none can
absolve it alone. The three run-level values are:

| verdict | meaning |
|---|---|
| `contaminated` | at least one instrument *measured* interference |
| `unknown` | nothing fired, but not every instrument could measure |
| `clean` | every axis actively said clean |

The important half is `unknown`. An axis with no measurement, or with too
little history to have a band, returns `unknown` — never `clean` — so "clean"
has to be earned by all three. Replaying the committed history through it, **no
record in the entire 24-run history grades `clean`**, which is the correct
answer: none of those runs was ever shown to be quiet, and one of them
demonstrably was not.

Wall clock is now measured (`QEMU_START_EPOCH`/`QEMU_END_EPOCH` around the QEMU
window only, stamped at the first `kill_qemu` so the harness's own log
processing is not counted as guest time) and stored as `wall_seconds`.
`--host-load=idle|loaded|unknown` is recorded as `host_load`, defaulting to
`unknown` — and `loaded` runs are excluded from every baseline and band by
`comparable_records()`, because a deliberately-poisoned control that silently
becomes a baseline would make the next honest run report its own recovery as a
suite-wide improvement.

**Deviation from the fix as written above, item 3:** the plan called for a
separate `bench/history-loaded.jsonl`. One labelled file was implemented
instead. A second file means every reader has to open two and any that forgets
silently drops the controls; a label in the one append-only file cannot be
missed, and `previous_for_host`/`report_run_position` now share a single filter
so the exclusion cannot be applied to one and not the other.

**CORRECTION to item 1 of the prescription above.** It said "a run with a
materially elevated stall count is CONTAMINATED regardless of the canary", on
the strength of the 3 → 8 move across the two boots. Implementing it against
the full history showed that does not hold. The 18 release records carry stall
counts of 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 5, 7, 7, 8, 9, 9, 13, 13, 15 — median
5, MAD 2 — so **8 sits at roughly the 75th percentile and is not
distinguishable from this host's ordinary behaviour**. A `median + 3·MAD` band
fires on the 13-, 13- and 15-stall runs and would *not* have fired on the run
that motivated the axis.

The band was left where the standard robust-outlier rule puts it rather than
lowered until the motivating run fires. Fitting a threshold to a single
observation is the mistake this file has had to undo three times
(`CANARY_TOLERANCE_PCT`, `DISPERSION_SUSPECT_RATIO`, the absolute A/B cycle
budget). The honest reading of the data is that dispersion is a *graded* and
rather noisy signal on this host — its floor is ~3 stalls, never 0 — and that
**the axis which actually separates that pair of boots is wall clock (160 s vs
365 s), not dispersion.** That is now recorded, which it was not, and it is why
recording it mattered more than tightening the dispersion band.

Consequences worth carrying forward:

- ~~The wall-clock band cannot fire yet: no stored record carries
  `wall_seconds`, so the axis correctly reports `unknown` and will stay there
  until `MIN_WINDOW_FOR_BAND` (6) comparable runs have one. This is a check that
  *cannot* fire today — the condition this file treats as equivalent to a check
  that passes — so it is called out rather than left to be discovered: until
  six timed runs exist, the wall axis is a recorder, not a detector.~~
  **RESOLVED 2026-08-19 by accumulation, and verified rather than assumed.** 61
  comparable release records now carry `wall_seconds` — ten times the window
  minimum — with median 130 s, MAD 17, floor 6.5 s and a resulting band of
  **181 s**. Replaying each record against the history that causally preceded
  it, the wall axis **fires on 8 runs, clears 59 and abstains on 22**: it is a
  detector, not a recorder. `test-bench-history.py` now carries the positive
  control that proves it (`test_the_wall_band_fires_on_the_real_history`),
  asserting both that it condemns at least one genuine run and that it clears
  more than it condemns — the same two-sided shape as the dispersion control
  below, and for the same reason. That is the **fifth** time in this project a
  freshly-written check has needed proof it can fire at all.
- The dispersion band *can* fire and provably does: `test-bench-history.py`
  exercises it against the real `bench/history.jsonl` and asserts both that it
  fires on at least one genuine run and that it does not fire on the majority.
  That positive control is the fourth time in this project a freshly-written
  check has needed proof it can fire at all.
- The `--bench` header's "The canary reports it as CONTAMINATED — believe it"
  now carries the one-sided reading in `boot-test.sh` itself, and the clean-
  canary line in the tool states the structural blindness rather than only the
  weaker sampling caveat. Stating the sampling limit alone implies the canary
  would catch host load if it sampled more often; it would not, at any rate.
