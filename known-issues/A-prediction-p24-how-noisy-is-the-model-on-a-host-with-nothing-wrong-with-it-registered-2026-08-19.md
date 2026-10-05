### [A] PREDICTION P24 — how noisy is the model on a host with nothing wrong with it? — registered 2026-08-19

**In short:** P23 penalised the model for flagging two benchmarks that no load
had touched. Before concluding the model invents disturbances, it is worth
checking the obvious alternative: that those two benchmarks really do disturb
the machine, all by themselves, every run. Nobody has ever run this suite with
no load at all and looked at what the model says. This registers what I expect
before doing it.

**The run:** `scripts/boot-test.sh --bench --host-load=idle`, five times, host
otherwise quiet. No spinners, no load window, nothing to detect. At ~136s each
this costs about twelve minutes for all five — P23's real cost, not the twelve
minutes per run that was assumed before it was measured.

**Why five and not one.** A single run cannot distinguish "position 72 is
special" from "one sample was noisy". The archived traces are genuinely
ambiguous: position 72 is elevated in three of four, but *not* in P22 run 3,
which is the one run whose load sat immediately beside it. One clean unloaded
run would prove nothing and one dirty one only slightly more.

#### Claims

- **P24(a) — the model is quiet on an idle host.** At least 3 of the 5 runs flag
  **zero** benchmarks. *Falsified if 3 or more runs flag anything at all.*
- **P24(b) — position 72 is not special.** The canary sample at position 72
  clears the 10% threshold in **at most 2** of the 5 runs. *Falsified at 3 or
  more* — which would make the two P23 "false positives" a real, repeatable
  property of `vfs_throughput_16k_write/read`, and P23(b) unfalsifiable as it
  was written.
- **P24(c) — no whole-run alarm.** At most 1 of the 5 runs reports
  `CONTAMINATED`. *Falsified at 2 or more*, which would mean the 25% spread
  tolerance is simply too tight for this host and every contamination verdict
  recorded to date needs re-reading.

#### Which way I actually lean, and why it is close

I am registering P24(b) **against** the self-contamination hypothesis I proposed
one entry above, because P22 run 3 is real evidence against it: its load sat at
positions 60-71, immediately adjacent, and position 72 still read a clean 5.16.
If those two VFS benchmarks reliably inflated the canary, run 3 is the run that
should have shown it most clearly, and it did not.

Against that, P23's own trace is hard to explain away: the load was released at
position 63, position 64 read clean, and 72 read 5.81. Load residue does not
skip a sample and come back.

So this is close to a coin flip and is registered as such. **That is the point
of registering it.** Had I written this up after the runs, either outcome would
have read as confirmation of whichever story the data told.

#### What each outcome licenses

| outcome | what it means |
|---|---|
| all three hold | the model is quiet when nothing is wrong; P23's 2 false positives were noise, and the 50% sensitivity finding stands alone |
| P24(b) falsified | the suite contaminates its own instrument; `grade-positional.py` needs a notion of benchmarks that are *expected* to disturb the canary, and P23(b) must be regraded as unfalsifiable rather than failed |
| P24(a) falsified | the model flags on an idle host, which makes every positional attribution it has ever produced suspect and is a far more serious finding than P23's |
| P24(c) falsified | the spread tolerance, not the model, is the thing that is miscalibrated |
