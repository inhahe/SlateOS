## §232 — Image identity is a relation, not a label, and a hash may not be overruled by a commit

**Date:** 2026-08-19
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The benchmark tool needs to answer "did these two measurements come from
the same kernel?" It used to answer by comparing the git commit each run was labelled
with, which was wrong in both directions and produced four false regression reports in
one afternoon (known-issues.md, 2026-08-19). It now hashes the kernel file that was
actually booted. The decision recorded here is what to do about the ~79 older runs that
have no hash: they are compared by commit as before, but a hash always wins over a
commit when one is available, and two hashes settle the question between themselves.

### The problem the obvious fix leaves behind

Hashing the booted ELF is not the interesting part; it is obviously right. The
interesting part is that the history already contains 79 records with no hash, and the
gate that consumes them — "has this same binary produced this movement more than once?"
— is worthless if it can only compare records written from today onward. A gate that
cannot fire is the same failure as a gate that fires wrongly.

So identity cannot be a string that each record carries and that is compared for
equality. A run hashed today and a clean, unhashed run of the same commit from last
week *are* the same image, but one is labelled `sha:eec7c6f373f19b6f` and the other
`commit:6e780afbc`, and those do not compare equal. The question has to be asked of a
*pair* of records, not derived from each separately.

### The decision

`same_image(a, b)`:

1. **Both hashed** → the hashes decide, and nothing else is consulted.
2. **At most one hashed** → fall back to: both clean (not `dirty`), both with a known
   commit, and the commits equal.
3. **Otherwise** → not the same image. In particular, unidentifiable never matches
   unidentifiable: two runs that both failed to read HEAD, or that were both measured
   with uncommitted changes, are not evidence about each other.

Rule 1 taking precedence over rule 2 is the part with a real tradeoff, and it is
deliberate. If two records carry differing hashes, a matching commit label does **not**
rescue them. On this project that is not pedantry: §228 records a benchmark moving
several-fold purely because its *code moved in the address space*, with identical
source. "Same commit" is a statement about text; the gate is about behaviour.

### The alternative, and why it lost

**Require a hash on both sides, full stop.** Cleaner, and it never asserts anything the
record cannot support.

*Against:* it retires the replication gate for every existing record, and the gate is
precisely the thing that stops a noisy run being written up as a regression — the
failure that motivated it happened *twice* before the gate existed. Reintroducing a
window of months during which the check silently declines to fire, in exchange for
removing an assumption ("a clean commit determines the bytes") that is true of every
build this project has ever made, is a bad trade. The assumption is also *checkable*:
once both sides are hashed, rule 1 supersedes it and any counterexample surfaces as a
`SAME COMMIT, DIFFERENT IMAGE` banner rather than as a silent mismatch.

*Also against:* the fallback is self-retiring. Every future run carries a hash, so rule
2 applies to a strictly shrinking set of comparisons and needs no deprecation plan.

### Consequence for the reader

The banner now names three states rather than asserting one of two. A comparison whose
commits match but whose images differ says so explicitly and says *how* it knows
(`DIFFERENT IMAGE`, both hashes printed, versus `UNKNOWN IMAGE`, neither pinnable).
The rule behind that: where the record cannot support a claim, print the ignorance —
never let a matching commit in the header line stand as unrebutted evidence that the
code was the same, because that is exactly the inference that produced the false
reports.
