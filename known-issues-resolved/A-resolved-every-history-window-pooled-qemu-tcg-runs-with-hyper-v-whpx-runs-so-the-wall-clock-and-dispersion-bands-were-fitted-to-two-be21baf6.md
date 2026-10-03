### [A] RESOLVED — every history window pooled QEMU TCG runs with Hyper-V/WHPX runs, so the wall-clock and dispersion bands were fitted to two populations whose medians differ by 3.5x in one direction and 30x in the other — 2026-08-19

**Status:** RESOLVED 2026-08-19. Found while proving the wall-clock axis had
become a live detector (see `B-CANARY-IS-BLIND-TO-HOST-DESCHEDULING` above);
the axis works, and the moment it started working it exposed this.

**In short.** The benchmark history decides whether a run was quiet by
comparing it against *comparable* earlier runs. "Comparable" filtered by host,
by build profile, by whether the machine was deliberately loaded, and by
whether the run was a tagged experiment — but **not by which virtual-machine
accelerator produced it.** QEMU has two, and they are not the same computer:
on one byte-identical kernel image (`kernel_sha 7a17cf6be2a1`), Hyper-V/WHPX
is ~3.5x *faster* than TCG on the median benchmark and ~10x faster at best,
while being ~30x *slower* on device-bound ones — a HPET read costs 13.5 us
under WHPX (each read is a VM exit to the hypervisor) against 450 ns under
TCG's inline emulation. Pooling those into one band is the same error as
pooling debug and release builds, which this file already fixed once, except
worse: the debug/release move is a multiple in one direction, and this one is a
multiple in *both*, so no scalar drift correction can rescue it.

**What it would have done.** Today the wall band over 61 comparable release
records sits at 181 s (median 130, MAD 17, floor 6.5). The two accelerators do
not overlap at all: the TCG layout-sweep arms run 114–132 s, the WHPX arms
162–180 s. So:

- As clean TCG runs accumulate, the band tightens toward ~149 s, and **every
  honest WHPX run then reports a wall-clock excursion** — attributed to host
  descheduling, which is what that axis means, when the real cause is that a
  different accelerator was used.
- Run the other way — a few honest WHPX runs enter a mostly-TCG window — and
  the MAD inflates until the band fires on nothing at all. A check that cannot
  fire is this file's recurring failure mode.

**Why it had not bitten yet, and why that is not reassuring.** All 14 WHPX
records also carry an `experiment` tag, so `record_experiment()` already
excluded them on other grounds. That is a property of how the WHPX sweep
happened to be invoked, not something the window logic guarantees. The first
un-tagged WHPX boot would have started poisoning the bands silently. Note also
that `arm_group_key` (the layout-artefact grouping) *has* partitioned by
`accel` since the field existed — so the codebase already held the correct
belief in one place and the wrong one in another, which is exactly the shape of
bug that survives review.

**The fix.** `comparable_records(records, host, profile, accel)` gained the
accelerator as a fourth filter, and `accel` is threaded as a required fourth
positional through every window consumer: `previous_for_host`,
`report_run_position`, `level_shift_window`, `placement_is_constant`,
`level_shifts`, `repeats_by_commit`, `mode_structure`, `mode_split_search`,
`values_for_binary`, `replication_verdict`, `report`, and `cmd_list`'s causal
replay. `--list` re-judges every stored row rather than printing the verdict
filed with it, so it needed the same partition or it would have shown the
defect to a reader in the view most likely to be believed.

**Required, not defaulted — deliberately.** `profile` used to default to
`debug`. That default was removed in the same change. A window selector that
silently picks a population when the caller forgets to name one is precisely
the failure being fixed here: on this host the unnamed-accelerator bucket holds
sixty runs, so the wrong answer looks like a full and healthy history rather
than an error. Both selectors now raise `TypeError` if omitted, and a test
asserts it.

**An absent `accel` is not TCG.** `ACCEL_UNRECORDED = None` is its own bucket
and groups only with itself. Folding it into TCG is tempting — 60 of the 61
records are unlabelled and would light up the band immediately — and it is
*provably* wrong: the first WHPX run on this host (`2026-08-19T16:15:09`)
predates the field, so an absent value is demonstrably not evidence of TCG.
The accepted cost is that the first *labelled* TCG run finds no history and
both banded axes abstain for ~5 runs. That blindness is made loud rather than
papered over: `accel_thinning_note()` states, in the verdict's own notes, how
many comparable runs the window holds, how many were excluded for predating the
field, how many for naming a different accelerator, and why. Silent blinding is
worse than loud abstention — the doctrine this file has arrived at three times.
Recorded as `design-decisions.md` §241.

**Evidence the fix is real, not just present.** Ten mutations of the production
file were run against the suite; all ten fail it. Two survived the first pass
and were only caught after the tests were strengthened, which is the whole
point of running the exercise:

- *"the thinning note is spoken even when nothing was thinned"* survived
  because the fixture for the stays-quiet case had no excluded neighbours, so
  the length guard was never the thing under test. The fixture now keeps the
  neighbours, which is the only version of the check that can tell the length
  guard from the neighbour guard.
- *"`--list` judges every record against one pooled history"* survived because
  nothing exercised `cmd_list` at all. It now has a test that builds a
  seven-run TCG history plus one ordinary WHPX run and asserts the WHPX row
  reads `unknown` (no history of its own) rather than `contaminated` (condemned
  by the TCG band).

**A falsification test, derived from the data rather than hand-picked.** Any
benchmark where `min(WHPX) > 4 * max(TCG)` across labelled release records
yields exactly `hpet_read`, `net_arp_lookup` and `net_ns_arp_lookup` — a
*structural* VM-exit signature, not a tuned threshold. Applied to the 60
unlabelled records it flags exactly one: the known 16:15 WHPX probe, which
doubles as the test's positive control. So the unlabelled window is genuinely
homogeneous, and the claim that it is TCG-like is now checked rather than
assumed.

**A second consumer, found by the tests rather than by reading.** Threading the
argument turned `comparable_records` into a required-argument function, which
made `scripts/grade-positional.py` fail loudly instead of silently — it builds
its per-benchmark baseline from the same window. That grader is *more* exposed
than the bands are, because everything it prints is a **ratio** of this run's
per-benchmark value to the baseline's. A uniform scale error would cancel in
those ratios and be harmless; the accelerator's is emphatically not uniform, so
a baseline from the wrong one *reshapes* the inflation curve and manufactures a
disturbance in whichever region the two accelerators disagree about most. It
now reads the accelerator from the graded run's own serial banner
(`bh.parse_accel`) rather than from a flag — a run that has to be *told* which
accelerator produced it can be told wrong. Its own regression test derives the
gate's null distribution from real history and now forms consecutive-run pairs
*within* an accelerator before pooling them; a pair straddling the boundary
does not measure between-run drift at all, and would have inflated the null
until the gate it calibrates could never fire.

**A restatement removed while in the area.** `comparable_records` and the new
`accel_window_thinning` each open-coded the host / profile / not-loaded
conditions that `measurement_mismatch` already expresses — the exact drift
hazard `design-decisions.md` §240 was written about, sitting inside the file
that documents it. Both now call it. A mutation that removed the delegation
from `accel_window_thinning` survived the first pass: nothing checked that the
excluded-neighbour *count* counts only neighbours. It now does — a record on
another host, in another profile, under load, or tagged as an experiment was
never a candidate for this window, and counting one would make the note claim
that a run is usable once you settle on an accelerator when in fact it is
excluded three other ways.

**Mutation ledger for this change: 14 mutations, 14 caught** — 12 against
`bench-history.py`, 2 against `grade-positional.py`. Three of the fourteen
survived a first pass and were only caught after the tests were strengthened.
