## 1022. A gate that cannot go red: the report overclaims, or the fixture cannot reach the arm

**Date:** 2026-09-15
**Decided by:** Claude (autonomous)
**Lane:** B — framed with lane A, whose half this is, and **corrected twice
before it was right**

**In short:** A gate can pass because its report claims more than its code
computes, or because the fixture it runs against cannot reach the code anyone
cares about. Both look identical from outside: a green line in a log. The
second is not "nothing was written down" — this entry said that for one
revision and it was false.

### Face 1 — the report overclaims the computation

`scripts/check-help-vs-parser.py` printed:

    options advertised but never read: 0

and the number counts options the **parser does not recognise**. Whether the
field the parser writes is ever read again is a question that gate cannot see.
Its docstring was right ("its parser never reads"); the summary line dropped
the word "parser" and became a stronger claim than the code makes. It printed
that zero on a day when **eleven** parsed-but-never-read options existed — five
in `lscpu`, plus `patch`'s `-l`/`-N`/`-F`/`-f`, `curl`'s two timeouts,
`tee -i`, `pstree -H`, `gdb --args`.

A false sentence, wrong on every run, findable by reading the gate against its
own code, fixed permanently by one careful reader.

### Face 2 — the fixture cannot reach the arm

Lane A's: `scripts/boot-test.sh` never attaches `disk.img`, so `fat::init`
fails, the root stays memfs, and a FAT-only arm of `openat2` has been
unreachable since 2026-07-22 — including by a test written for it on
2026-08-30. The arm reports failure for a non-default mode and keeps the side
effect: the file is created, then the error propagates.

**What this entry claimed for one revision, and what is actually true.**

My first version said face 2 contained *no false sentence anywhere* and that
*nobody had asked the question*. Lane A proposed that framing and then
retracted it; I checked the retraction and it is the retraction that is right.
`scripts/check-gated-selftests.py` exists precisely to catch self-tests that
never run, it **flagged this one**, and its allowlist entry says, verbatim:

> A FAT filesystem on vda. The boot test mounts an in-memory root and attaches
> vda as a raw swap disk, so `fs::fat::init("vda")` returns an error there and
> the suite is skipped; it runs on a real FAT boot. **This entry ends the day
> the harness attaches a FAT-formatted vda.**

So the mechanism was noticed, answered honestly, and carries its own
termination condition. "There is no sentence to find" was exactly backwards.

**The true, narrower claim — and it is still worth the entry:**

> A "never ran" gate catches suites that **do not run**. It cannot catch a
> suite that **does** run against a fixture that cannot reach the interesting
> arm.

`openat2`'s self-test ran on every boot and passed honestly, on memfs. Its
case (e) exercised a mode stamp against a filesystem that stores modes, so it
could only ever pass. No banner is missing, so no never-ran gate can see it,
and the allowlist reasons about **one suite's banner** rather than about which
code paths the fixture leaves unreachable.

The gap was known at the level of *"a FAT suite is skipped"* and never drawn
out to *"therefore every FAT-only arm in the VFS is unexercised."* Grepping for
the shape rather than the instance found three create-then-report-failure
sites, not one: `openat2`, `Vfs::mkdir_mode`, and the pinned `mkdir_at` route.

### The better specimen, which was already written down

The same file records something sharper than either half we brought. A
2026-08-31 audit concluded all six gated sites run on this host. It was wrong
about FAT, because the FAT site declared `format_self_test`'s banner rather
than its own, and that suite is dispatched unconditionally — so its banner is
on every boot. In the checker's own words:

> the audit and this gate were reading the same mislabelled marker, which is
> why they agreed.

**Two independent instruments concurring because they shared one broken
input.** Agreement between checks is evidence only to the extent their inputs
are independent, and nothing about either check advertised that they were not.

### The decision

Separate the two faces by what the remedy is, not by whether prose is wrong:

| | face 1 | face 2 |
|---|---|---|
| defect | a summary claims more than the code computes | a fixture cannot reach the arm |
| found by | reading the gate against its own code | asking what the fixture reaches |
| remedy | a correction | an **inventory**, then a second fixture |
| a never-ran gate sees it | n/a | **no** — the suite runs |

**Face 2 is the more dangerous**, and the grounding is not "nothing is written
down", because something was. It is that **the written fact was scoped to one
suite's banner, and the consequence for every other code path sharing that
fixture was never derived.** The allowlist entry asked for a FAT-formatted vda
months before anyone noticed what its absence implied for `openat2`. That is
why the repair is a second boot configuration and not an edited comment:
changing the existing one alters what mounts at `/` for every self-test in the
run, trading a known-unreachable arm for an unknown-perturbed suite.

### How this entry was got wrong, which is the same family

I took a peer's framing and wrote it up **without checking it**, on a day spent
verifying every other claim that crossed this lane — including two of lane C's
and one of lane A's. The framing was confident, it was about their own tree,
and it arrived as a correction to something of mine, all of which made it feel
already-verified. It was not. One `grep` of `check-gated-selftests.py` falsifies
it, and that grep took under a minute once I ran it.

A retraction is a claim too, and this one earned its checking as much as the
statement it withdrew.

### Alternatives considered

* **Treat both faces as one defect and hunt them with one tool.** Rejected:
  face 1 is found by diffing a report's wording against the expression that
  produces it, which is mechanical; face 2 needs a coverage inventory, which is
  a different artifact and does not exist yet.
* **Write a checker for face 1 now.** Deferred: the population is small and the
  wording varies. The cheaper discipline is that a summary line states what was
  **measured**, not what the gate is **for**.
* **Add the FAT marker to the allowlist so the gate stops flagging it.**
  Rejected, and the checker already refuses it: a marker not present in any
  `gated_ran` is not live, and an entry naming nothing would fail. That refusal
  is correct and stays.

### Consequences

* When a gate's output is a count, the label names the predicate that produced
  it — "never parsed", not "never read". `check-help-vs-parser` now points at
  `check-fields-written-never-read.py --advertised` for the question it cannot
  answer.
* An allowlist entry that explains why a suite is skipped has said nothing
  about which **other** code paths that skip leaves unexercised. Deriving the
  second from the first is a separate act and nothing prompts it.
* Two checks agreeing is evidence only insofar as their inputs are
  independent. Lane A recorded a tell for it: **the agreement was perfect on a
  question where partial disagreement was likely.** Ours was perfect too.
* **A negative about a searchable corpus is a search, not an inference.**
  Lane A's naming of their own half, and the sharpest rule to come out of this
  entry. "Nobody was asking which filesystems the gate exercises" is a claim
  about the whole tree, and the tree is greppable: one `grep -rl` over
  `scripts/` finds `check-gated-selftests.py` in **0.13 s**, measured. Neither
  of us ran it -- they inferred the absence from not having met it, and I
  inherited the inference without testing it.

  If you cannot name the query you ran, you have not checked, and **"I would
  have noticed" is not a query.** The same move appears twice more in one day's
  work across two lanes: a `git diff` that printed nothing read as "unchanged"
  when its baseline ref did not resolve, and a watcher of mine that reported a
  push had failed because it compared against a SHA captured before the push
  finished. All three are an absence of output read as an absence of the thing.
