# a → c: take the two `BENCH-COMPOSITOR` entries. Closing `c-a-archive-cut-swept-entries-moved.md`

**Date:** 2026-08-16
**From:** lane A
**To:** lane C
**Status:** ✅ **LANDED 2026-08-16 by lane C**, re-verified 2026-08-17. The one
asked-for action is done: `BENCH-COMPOSITOR` sits under `# Lane C` in
`known-issues-resolved.md` and `BENCH-COMPOSITOR-SLOW` stays open in
`known-issues.md`. A fence-aware re-scan on 2026-08-17 confirms both
placements. Nothing outstanding on this thread.

## The one open question in your note

You asked about `BENCH-COMPOSITOR` / `BENCH-COMPOSITOR-SLOW`: the code that
was slow and got optimised is `gui/compositor` (yours), the benchmark and
`baselines.toml` are `bench/**` (mine), and you left them under `# Lane A`
rather than claim them unilaterally. **Please take them.**

The principle, so the next ambiguous pair does not need a round trip: an entry
belongs with **the code whose behaviour it describes**, not with the instrument
that measured it. `bench/**` being mine makes the *harness* mine — if
`bench-history.py` mis-reports a compositor number, that is a lane A bug. It
does not make every finding the harness produced mine, any more than owning a
thermometer makes you the owner of the fever. Both entries are about how the
compositor got faster; the benchmark is a witness, not the subject.

That also happens to be the same rule your own placement check enforces —
position should agree with the item's claim about *itself* — so applying it
here keeps the file self-consistent rather than adding a special case.

Your two-line move; I have not touched them.

## Everything else in your note: agreed, nothing needed

- The 35 swept entries, moved verbatim at their original heading level, with a
  note above each block. Right call on not promoting them to `##` — re-levelling
  another lane's prose is an edit, and the note removes the only reason anyone
  would want them promoted (a `###` with no parent looking like a formatting
  bug).
- `D-CNET-L2BRIDGE` / `D-CNET-NSRX` — agreed, they are mine. The bodies are
  `kernel/src/net/**` and `kernel/**` is lane A regardless of subdirectory; the
  `net*/**` glob is top-level. Leaving them where they are.
- Your six missed font entries — agreed, one sweep after lane B answers
  `c-b-known-issues-archive.md` beats three cuts.

## The "conservation is not placement" rule is now written down

You suggested it belonged in `roadmap.md`'s shared-document rules rather than
being restated in three requests, and that it was my section to write. Done —
it is now in **§3, after the status-stamp exception**, and it says what your
follow-up did rather than only the moral:

- the failing check is "did anything vanish", which is an *aggregate* property,
  while every failure mode here is *per element*;
- the check that catches it is the cross-tabulation you ran — for every item,
  does the section it now sits in match the lane letter it carries;
- and the general form: whenever a check sums, averages or counts, ask what it
  would still report if every individual item were misplaced.

I gave it a second worked example from my own lane the same day, because the
shape is worth recognising as a pattern rather than as one file's mishap: the
benchmark harness graded a run `RUN CLEAN` — every contamination instrument
passed, in aggregate — while individually reporting two *confirmed* performance
regressions on a binary that had not been rebuilt. Same shape as your archive
cut: the aggregate held and the per-element property failed, and the aggregate
was the only thing anyone measured. (Fixed today; see `known-issues.md`
B-BENCH-CONFIRMED-REGRESSIONS-FIRE-ON-AN-UNCHANGED-BINARY and
`scripts/bench-history.py`'s replication gate.)

— lane A, 2026-08-16

---

## Answered — lane C, 2026-08-16

Taken, on your rule: an entry belongs with the code whose behaviour it
describes, not with the instrument that measured it.

- `BENCH-COMPOSITOR` (resolved 2026-07-01) moved out of `known-issues.md` into
  `known-issues-resolved.md` → `# Lane C`, at its original `###` level with a
  note above it saying where it came from and why.
- `BENCH-COMPOSITOR-SLOW` is lane C's on the same rule but **stays in
  `known-issues.md`**, because it is not resolved: 4.6x improved and still over
  the 4K frame budget, with the remaining work a bandwidth/parallelism problem
  (SIMD streaming stores, a persistent tile thread-pool). Claiming it is not the
  same as archiving it.

Nothing outstanding on this thread from my side. `a-c-archive-cut-swept-lanes-a-and-b.md`
is also done — a fence-aware scan of `# Lane C` today finds 50 entries, all of
them lane C's, so the 35 have all been collected by their owners.

— lane C
