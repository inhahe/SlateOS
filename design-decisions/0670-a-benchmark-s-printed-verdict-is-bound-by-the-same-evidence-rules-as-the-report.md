## §670 — a benchmark's printed verdict is bound by the same evidence rules as the report that scores it

**Date:** 2026-09-01
**Lane:** A
**Decided by:** Claude (autonomous)

**In short:** a benchmark in the kernel measures something, then prints an
English sentence saying what the measurement means. On one run the measurement
was garbage — the machine's speed changed halfway through, so the first and
second halves of the run disagreed by 92% — and the sentence was printed anyway,
confidently, and was wrong in three separate ways. The tooling that reads these
runs *had* already thrown the same numbers out as unusable, in the same log. The
decision is that the sentence must obey the rules the tooling obeys: say nothing
when the measurement is void, never narrate a physically impossible number as a
finding, and never assert a cause the run did not measure.

### Why a printed verdict is not just a convenience

`bench_vfs_readdir_root_cost` exists because an earlier investigation lost two
days to acting on an unmeasured hypothesis. Its whole purpose is to replace
reasoning with measurement, and it ends by printing a one-line conclusion so the
next reader does not have to re-derive it from five numbers.

That last part is the dangerous part. A table of numbers carries its own
caveats — a reader who sees `92% UNSTABLE` next to a figure discounts it. A
sentence does not: "STATS-DOMINATE — mount parenthood costs 18422ns and the 5
measured stats account for 151% of it" reads as a finding no matter what
produced it. The convenience that makes the line worth printing is exactly what
makes a wrong one worse than no line at all.

### The three ways one run got it wrong

All three came from a single boot, the first after the `finish_listing` fix.

1. **It concluded from samples the host-side report had discarded.** `run`
   already cross-checks each measurement window's two halves against each other,
   and `bench-report`'s `MEASUREMENT VOID` class refuses to score a benchmark
   whose halves disagree. On this boot `mp_parent` and `mp_elsewhere` were 92%
   and 82% apart, the report correctly voided them — and four lines earlier in
   the same log, the kernel's own verdict had already announced a conclusion
   drawn from them.

2. **It narrated an impossible number as a finding.** It reported that adding
   five mounts made an unrelated listing *faster* by 26%. That cannot happen:
   `resolve_mount` does a longest-prefix scan whose length is monotonic in the
   number of mounts. A negative there is not a small effect, it is proof the arm
   measured noise — and it was being printed in the same voice as the real
   results, and fed as a correction term into a later arm.

3. **It described the tree as it had been, not as it was.** The verdict said
   "the known-issues fix is the right one and should recover most of this" about
   a binary that already contained that fix. The `submount_stat` arm measures the
   by-path stat the fix *removed*; once removed, five of them cost more than the
   delta they used to be part of, which is why the fraction read 151%.

### The decision

Three guards, and one deliberate non-guard.

- **Void guard.** The ladder checks `split.is_unstable()` on every arm it rests
  on and prints `VOID` with the split figures instead of a verdict. The rule is
  that the in-kernel sentence may not claim more than the host-side report will
  score.
- **Impossibility guard.** A negative mount-table delta is named as noise, and
  the per-mount unit price derived from it is withheld from the arm that
  subtracts it, rather than laundered into a figure that looks corrected.
- **Tense.** Every surviving branch is worded so none presupposes the fix is
  unapplied. The two that should now be unreachable say so, and say to check the
  split figures before believing them.
- **`explained` is deliberately NOT clamped to 100%.** This is the non-guard, and
  it is the interesting half. A percentage over 100 looks like a bug and the
  reflex is to clamp it. But while the by-path stat was inside the loop, five of
  them could not exceed the delta they were part of; that they now do is the
  measurement reporting that the call has left the loop. Clamping would have
  destroyed the one signal the arm was added to detect — and did, in effect:
  the run reported `STATS-DOMINATE` where the truth was `STATS-REMOVED`.

### The alternative, and why not

The obvious alternative is to delete the printed verdict and leave the numbers,
on the grounds that a number cannot lie about itself. That is genuinely safer,
and it is what most benchmark suites do.

Rejected because the numbers here are not self-explanatory: the finding is a
*difference between two arms* that only means anything once mount-table growth
has been subtracted, and a reader who does not know that will read `parent` as
the cost of listing a directory. The sentence is what carries the experimental
design. Deleting it would trade a verdict that can be wrong for a table that is
reliably misread — and the entry this benchmark serves exists because someone
misread a table.

The narrower alternative — keep the verdict but drop the `submount_stat` arm now
that its call is out of the loop — was rejected for the same reason the arm was
added: it is the yardstick for what the fix removed, measured fresh every boot
instead of quoted from a constant. `metadata_resolved` is still live code and
still what any ordinary caller pays to stat a mount point. A constant quoted
instead of measured is the exact failure this benchmark was built to end.

### Consequences

- Boots where the host is loaded produce `VOID` rather than a confident wrong
  sentence. This is a real loss: a contaminated run now yields no verdict at
  all, and contamination is common on this host. Accepted — the run's numbers
  are still recorded and still enter the history, so nothing is lost except a
  claim that should not have been made.
- The `STATS-DOMINATE` and `STATS-PARTIAL` branches are now assertions that a
  regression has occurred, not descriptions of a healthy state. If either fires
  on a quiet host, `finish_listing` has gone back to resolving paths.
- The rule generalises beyond this benchmark: any in-kernel line that interprets
  a measurement is bound by the evidence rules of the tooling that scores it. No
  other benchmark prints a verdict of this kind today; if one is added, this is
  the standard it has to meet.
