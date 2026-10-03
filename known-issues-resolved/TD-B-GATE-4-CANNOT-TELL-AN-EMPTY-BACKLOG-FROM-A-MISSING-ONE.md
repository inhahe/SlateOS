## TD-B-GATE-4-CANNOT-TELL-AN-EMPTY-BACKLOG-FROM-A-MISSING-ONE (lane B)

**Filed:** 2026-09-03 by lane B, out of gate 6's `--head` conversion.
**Status: FIXED 2026-09-03**, the same day, by the plan below. `_no_corpus`
became `_inputs_missing(tree, needs_baseline)` — the same name and shape as
`scripts/host-errmsg.py`'s, because these are one rule enforced at two gates
and two spellings of one rule is one rule that drifts. `--check` on the real
tree is unchanged (`4 finding(s); 0 not in the baseline; 0 baseline line(s) now
stale`, exit 0) and `--selftest` still reports 7/7.

**The case that pins it needed a second draft, and the first one is the more
useful thing to record.** Written against the existing fixture — whose baseline
is the empty `# nothing known-panicking yet` — it caught the mutant, but for the
wrong reason: with nothing forgiven, a missing baseline is a *silent* false pass
(exit 0), so the status assertion fired while the assertion that no bin is
accused would have held equally against a checker with no guard at all. The
fixture now forgives a real finding, so removing the baseline turns that finding
into an accusation — and the mutant's failure is the documented harm rather than
a proxy for it: exit **1** with `tool.rs` named as new. Three assertions fire
where two did.

The original report follows unchanged.

**In short:** pre-push gate 4 forgives four known-bad utilities by listing them
in a file. If a commit ever moves or deletes that file, gate 4 does not say so —
it reads the missing file as "nothing is forgiven" and refuses the push, once
per forgiven utility, blaming four utilities the author never touched. The
message it prints talks about command-line argument handling; the actual fault
is that a file moved. Gate 6 had exactly this and now checks for it; gate 4 does
not.

**Where:** `scripts/argv-utf8.py` — `load_baseline` returns `set()` when
`tree.read_text(BASELINE_REL)` is `None`, and `main` guards only the *corpus*
(`_no_corpus`), never the baseline. `scripts/host-errmsg-baseline.txt`'s gate
does both, in `_inputs_missing`.

**Why it is not merely noisy.** A finding is a claim about the author's code,
and this one is false in both directions:

- **On a dirty tree:** every baselined bin reads as NEW, so the push is refused
  with gate 4's full refusal text over bins nobody edited.
- **On a clean tree:** the same read makes every baseline line *stale*, so the
  ratchet's shrink-only report claims the backlog was fixed by a commit that
  fixed nothing.

Neither is silent, which is the one mercy — but `scripts/run-checker.sh`'s whole
argument is that a gate spending its credibility on a verdict it did not reach
is the worst thing a gate can do, and this is that.

**The fix**, mirroring gate 6 rather than inventing one:

1. Give `argv-utf8.py` the second half of the guard — refuse when the baseline
   is unreadable *and* the mode actually reads it — and exit **2**, not 1, so
   `run_checker` classifies it as no-verdict rather than as a finding.
2. Exclude `--write-baseline` (it creates the file) from that guard, and pin the
   exclusion with an assertion, or the next person to widen the guard silently
   breaks bootstrapping.
3. Add the honour-head case: commit a tree with the baseline moved away, restore
   it on disk, and require exit 2 from the `--head` arm and a normal verdict
   from the disk arm — the gate-6 case
   `case_gate6_a_baseline_absent_from_the_tree_is_not_a_pile_of_new_findings`
   transposed.
4. Mutation-verify by deleting the new guard and confirming the case fails. A
   guard added without a surviving mutant to kill is a guard nobody has tested.

**If it is never fixed:** nothing breaks until someone moves
`scripts/argv-utf8-baseline.txt`, at which point every lane's pushes are refused
with a false accusation until somebody reads the checker. The exposure does not
grow with time. Gates 5, 8 and 11 are unconverted and should be given both
halves of the guard when they are converted, rather than inheriting gate 4's
half.
