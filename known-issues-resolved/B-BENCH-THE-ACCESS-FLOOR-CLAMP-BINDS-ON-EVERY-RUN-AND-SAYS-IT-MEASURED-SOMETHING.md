### [A] B-BENCH-THE-ACCESS-FLOOR-CLAMP-BINDS-ON-EVERY-RUN-AND-SAYS-IT-MEASURED-SOMETHING — 2026-08-15 — 🔧 FIXED in `90457f629`; both constants re-derived per build profile (see RESOLVED section at the end of this entry)

**In short:** the benchmark suite calibrates two of its budgets against "how
much does one memory access cost on this machine", measures that as **5
cycles**, then quietly throws the measurement away and uses a hard-coded
**100** instead — on every run, without ever saying so. The line it prints
looks like a successful calibration and reads `measured=5.0 ... budgets below
are multiples of this`, where "this" is 100, not 5. So both budgets are 20x
looser than they claim to be, and they have never once been calibrated.

**Where:** `kernel/src/bench.rs`, `let floor = core::cmp::max(measured_cycles.unwrap_or(0), 100);`
(the `access_floor` binding, ~line 1432). Consumers: `mmio_suspicion =
access_floor * 4` (`fast_cpu_index` PASS/SLOW, ~line 1513) and `access_floor *
OWNER_TAG_BUDGET_ACCESSES` (frame-owner tagging, ~line 1658). It is also the
divisor in `accesses()`, which prints "N accesses" figures in the report.

**Evidence it binds every time.** Across every run in `build/` that used the
current 1024-stores/window calibration:

| run | measured | floor used |
|---|---|---|
| 4 runs, 1024 stores/window | 5.0, 5.0, 5.1, 5.1 cycles/store | 100 |

`max(5, 100)` is 100 in all four. There is no recorded run on the current
calibration where the clamp did not bind, so no budget verdict the suite has
ever printed was calibrated to the machine it ran on.

**Why the loud `UNMEASURED` path does not catch it.** That path fires only when
the measurement *fails* (arms did not separate, or the scale check rejected
it), and it says exactly the right thing: "falling back to the arbitrary clamp
... verdicts below are NOT calibrated". The case here is the opposite and worse
— the measurement **succeeded**, was believed, and was then discarded anyway by
a clamp whose own comment claims it exists only for the degenerate
`unwrap_or(0)` case. A clamp that binds on a good measurement is not a guard;
it is a silent override.

**The root cause is a units/quantity confusion, not the constant.** Two
different physical quantities are being asked of one variable:

1. **Cost of one memory access** (~5 cycles here). This is what is measured,
   and it is the right divisor for `accesses()` — "this delta is worth N memory
   accesses" is a meaningful sentence.
2. **The noise floor of a single-shot measurement** (~100-200 cycles here).
   This is what the *budgets* actually need, and the comment at the
   `mmio_suspicion` site says so in as many words: an absolute 200-cycle budget
   "reported SLOW on every healthy boot ... because 200 is below this harness's
   floor for a single memory access — the nop baseline alone wanders by more
   than that between adjacent measurements".

The 100 is a hand-tuned stand-in for quantity 2 wearing quantity 1's name. That
is why it cannot be simply deleted: dropping to the true 5 would make
`fast_cpu_index`'s budget 20 cycles, far below the measurement noise, and it
would report SLOW on every healthy boot — the exact bug the clamp was added to
paper over.

**Proper fix** (both halves, neither sufficient alone):

1. **Measure quantity 2 instead of hard-coding it.** The A/B already runs
   `CANARY_ROUNDS` interleaved rounds; the spread of the nop arm across those
   rounds *is* the single-shot noise floor, and it is free — it is already
   being computed and thrown away. Budgets become multiples of a measured
   dispersion, and the two quantities stop sharing a variable.
2. **Make the clamp announce itself.** Whenever the fallback is used at all —
   whether because the measurement failed or because it was overridden —
   the run must say so and its budget verdicts must be marked uncalibrated,
   the same treatment the `UNMEASURED` branch already gives. Three outcomes
   (measured / clamped / unmeasured), never two.

**Consequence of leaving it:** the two budget checks cannot fail on this
harness for any realistic regression, so they are decorative. Nothing else in
the suite depends on the floor, and the SCORE lines that gate `BENCH_OK` do
not, so this is a silently-dead check rather than a wrong number — which is
the failure mode this project keeps rediscovering: a check that cannot fire is
indistinguishable from a check that passes.

**Fix landed — `90457f629`.** Half 2 went in as written above: three outcomes
(`measured` / `CLAMPED` / `UNMEASURED`), and the CLAMPED branch states plainly
that the budgets are looser than the machine warrants, so a PASS is weak
evidence while a SLOW still counts.

Half 1 went in **differently from the proposal above, and the difference
matters**, so the reasoning is recorded here rather than lost:

- *Proposed:* measure quantity 2 directly, as the dispersion of the nop arm
  across the interleaved rounds — free, since it is already computed and
  discarded.
- *Implemented:* measure the cost of a **scattered** access —
  `measure_scattered_access_cost` walks a 512 KiB buffer at a 4 KiB stride, a
  distinct guest page per store, so each store pays its own softmmu lookup the
  way real allocator code does.
- *Why the change:* the proposal accepted the entry's own framing that the
  budgets want "the noise floor of a single-shot measurement". They do not.
  They want *the cost of the kind of access the code under test actually
  makes*, which is a physical property of the workload, not of the instrument.
  The nop-dispersion figure would have been an honest measurement of the wrong
  thing — a number that moves when the harness gets noisier and stays put when
  the memory system gets slower, which is backwards for a budget. The
  scattered cost satisfies the noise constraint too (it is one to two orders
  of magnitude above the hot cost, hence well clear of the ~200-cycle wander),
  so one honest measurement discharges both requirements instead of trading
  one confusion for another.
- The clamp is *retained* as the noise guard, because that is the one job the
  entry correctly identified for it — but it is now a floor that announces
  itself rather than a silent override, so a machine where the scattered cost
  genuinely came out under 100 cycles would say so instead of pretending.

The scattered measurement carries its own scale-invariance check, which
**halves** the store count rather than doubling it: doubling would run past
the end of the 512 KiB buffer and wrap onto already-resident pages, quietly
re-measuring the hot case and certifying it as scattered.

**Still open — the two budget constants.** `mmio_suspicion`'s `4` and
`OWNER_TAG_BUDGET_ACCESSES`'s `150` were both sized against the clamp, so each
has always meant a flat cycle count (400 and 15000) wearing an "N accesses"
label. Both are flagged `PENDING RE-DERIVATION` in the source. They are not
guessed at here because re-deriving them needs a boot that prints the measured
scattered floor, which had not run when this was written. The arithmetic is
already in the source comments and points the same way in both cases — the
`fast_cpu_index` comment says a healthy lookup "is one access or less" and
healthy boots measure 274-282 cycles, i.e. about *one* scattered access, not
four; the owner-tag comment reasons in "~16 accesses at TCG's
few-hundred-cycles-each" while the code divided by a 5-cycle one. Until they
are re-derived both budgets are merely loose, which is the direction that
cannot manufacture a false alarm.

#### RESOLVED 2026-08-15 — both constants re-derived, and the cause was a *profile* split, not a floor

The "pending re-derivation" above assumed the two budgets just needed
restating in the corrected (scattered) unit. That was the wrong diagnosis.
Re-deriving them from the recorded boots turned up something the unit story
does not explain:

| check | healthy **release** | healthy **debug** | old budget | old budget vs worst release |
|---|---|---|---|---|
| `fast_cpu_index` | 4-10 cycles (n=8) | 188-420 | 400 (`floor*4`) | **40x too loose** |
| `page_alloc_free_owner_ab` | 42-246 cycles (n=9) | 7660-12708 | 15000 (`floor*150`) | **61x too loose** |

The right-hand columns are the finding. **The 7660-11288 figures quoted in
the source comment as the healthy range were DEBUG boots**, and the comment
did not say so — the same kernel is ~40x slower in debug (`page_alloc_free`
is ~1330 cycles in release and ~52000 in debug). One constant was being asked
to span both profiles, so it was sized for debug and release lost: in release
neither check could fire at all. A check that cannot fire is indistinguishable
from a check that passes, which is why both had reported PASS forever.

**Fix:** both budgets are now absolute per-profile cycle counts selected on
`cfg!(debug_assertions)` — `mmio_suspicion` 100 release / 2000 debug,
owner-tag 1500 release / 40000 debug — and are no longer multiples of
`access_floor` at all. Each line also prints `[{} profile]` so a surprising
verdict can be attributed to the branch taken rather than to the code under
test. Verified on a release boot: `fast_cpu_index: PASS (8 cycles, limit 100
cycles [release profile])`, `page_alloc_free_owner_ab: PASS (92 cycles, limit
1500 cycles [release profile])`.

Note the second-order consequence: `access_floor` no longer feeds **any**
verdict. Its only remaining consumer is the display-only "N accesses" figures.

#### B-BENCH-THE-UNMEASURED-WARNING-VOIDS-VERDICTS-IT-NO-LONGER-GOVERNS — found and fixed in the same session

Caught by reading the verification boot's own log rather than its exit code.
With the budgets now absolute, the floor's failure message was still saying:

> Falling back to the arbitrary clamp of 100 cycles: budget-based verdicts
> below are NOT calibrated to this machine and **must not be read as
> findings**.

On that boot the scale check legitimately rejected the measurement, so this
printed — and the two lines immediately below it were sound absolute-cycle
verdicts that the reader was being told to discard. This is the exact mirror
image of the bug the entry above documents: **a warning that taints valid
findings trains the reader to ignore the instrument just as effectively as a
budget that cannot fire.** Both end in a verdict nobody acts on.

**Fix:** all three `memory_access_floor` messages (measured / CLAMPED /
UNMEASURED) now scope their claim to the "N accesses" figures, which are all
the floor still feeds, and state explicitly that the PASS/SLOW verdicts are
absolute per-profile cycle counts and still hold. The CLAMPED case also now
names the *direction* of its error (figures understated, because a bigger
divisor yields fewer accesses) instead of the previous vague "LOOSER than this
machine warrants".

**Generalisable:** when a consumer is removed from a shared calibration, the
calibration's *error messages* are part of its interface and go stale with it.
Grep the failure text, not just the call sites.
### A trailing `| tail` swallows the exit code too — and hides the log while it runs — 2026-08-15

Follow-up to "A trailing `tail` swallows the exit code the notification
reports". That entry says to make the command under test the **last** command in
the chain. That is not enough, because it is satisfied by:

```
python scripts/run-timeout.py 900 cargo test -p compositor ... 2>&1 | tail -50
```

`cargo` *is* the last command written, but a pipeline's exit status is the exit
status of its **last element**, so the notification reported `tail`'s 0. This
was reintroduced twice in one session by an agent who had written the original
entry the session before, which is the reason for restating it.

Two things are wrong with the pipe form and only one of them is the exit code:

1. **The status is `tail`'s.** The rule has to be stated as *the process whose
   status you care about must be the last element of the last pipeline* — not
   "the last command", which reads as satisfied by the above.
2. **The output file stays empty until the job ends.** `tail` cannot emit
   anything until its input closes, so the incremental log — the entire reason
   `run-timeout.py` streams and heartbeats — shows nothing while the job runs.
   Checking on a long build mid-flight returns an empty file, which reads as a
   hang.

Both vanish if the pipe is simply dropped. `run-timeout.py` already writes to
the harness's output file; use `Read`/`tail` on **that file** afterwards instead
of filtering in the pipeline. Reserve pipes for foreground commands whose status
does not matter.

*The 18 entries below are lane A's. They arrived in lane C's section by
accident: the archive cut of 2026-08-15 was made on `##` boundaries, and these
are `###` entries that happened to sit after a lane C `##` heading in the
append-only `known-issues.md`, so conservation carried them but placement did
not. Lane A reported it in `requests/a-c-archive-cut-swept-lanes-a-and-b.md`
and lane C moved them here on 2026-08-16 — verbatim, at their original heading
level, with no edit to their text.*
