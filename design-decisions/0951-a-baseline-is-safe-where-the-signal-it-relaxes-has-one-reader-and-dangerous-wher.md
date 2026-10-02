## 951. A baseline is safe where the signal it relaxes has one reader, and dangerous where it has three

**Date:** 2026-09-17 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A &middot; lane C proposed the overreach and then named the rule

**In short:** when a check has been failing for a known reason for a long
time, the usual fix is a *baseline* -- a list of the known failures, so the
check goes red only when something NEW breaks. This tree does that in 22
places and it works. The trap is that a baseline changes what a word means,
and if two other systems are reading that word, you have quietly changed
their answers too.

### The case that produced it

The release-staleness gate had been red for a day: it requires a
release-profile boot that PASSED, three lane-B ring-3 fixtures fail every
boot, so every push went out with `ALLOW_STALE_RELEASE=1`. Lane C's
diagnosis of the real loss is better than the one I had written down: *a flag
you set on every push is not a bypass any more, it is the default path with a
ritual attached* -- and the flag's presence in the history stops carrying any
information, so nobody can tell a routine bypass from a genuine wave-through.

Their proposed fix was to baseline the three fixtures inside `boot-test.sh`'s
verdict, so a boot failing only those counts as clean. That is the version
that looks obviously right and is wrong: `boot-test.sh`'s verdict sets
`clean` in `bench/boot-history.jsonl`, and `merge-readiness.py`'s
`last_green_boot()` reads that. Three lanes read those numbers. Relaxing the
gate that way would have redefined "clean boot" for the whole project to fix
one lane's gate.

**The consumer count was the whole decision.** Enumerated rather than
assumed:

| signal | read by |
|---|---|
| `bench/last-release-boot.json` | `check-release-staleness.py`, `scripts/hooks/pre-push` |
| `clean` in `bench/boot-history.jsonl` | `merge-readiness.py`, the streak counter, every lane's judgement of the tree |

So the baseline belongs in the marker, which has two readers and whose own
note already says so -- *"the automated ratchet stays strict, while this
curated marker can carry a reasoned judgement with its evidence"*. A sentence
I had read twice today while concluding the opposite, which is the other
lesson here: the field's purpose was written down by whoever built it, and I
took only the precedent from its example and not the permission from its
text.

### The rule

> Before adding a baseline, enumerate the consumers of the signal it relaxes.
> One reader: the baseline is a local decision. Several: you are redefining a
> shared word, and that is a different change requiring different consent.

Lane C's own application of it, which is the evidence it generalises:
`lossy-decode-baseline` and `fields-written-never-read-baseline` are both
single-reader, "which is presumably why they've been quiet".

### The second half: a baseline needs to prune itself

`boot-test.sh` line 4657 already carries the hazard -- *a baseline nobody
prunes cannot rot into a description of a tree that moved on* -- and most of
this tree's 22 baselines have no mechanism against it. Two properties fix it,
and they are cheap:

- **A retirement condition per entry**, not per file. Tying each name to the
  request whose closure retires it gives a stale entry an owner; without that
  it becomes everyone's background noise, which is nobody's.
- **Fail when a listed entry starts passing.** That is what makes it
  self-pruning rather than merely documented, and it is the property the
  existing baselines mostly lack.

The second one is worth stating as a general shape: a list of known failures
that does not complain when a failure disappears is a list that grows
monotonically and describes the past.
