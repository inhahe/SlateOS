## 651. The never-running-self-test gate asks the boot history, not the source: 100% of the last N ≥ 10 boots, with an allowlist that fails in both directions

**Date:** 2026-08-31
**Lane:** A
**Decided by:** Claude (autonomous), lane A

**In short:** A test can print `SKIP: <name> (<reason>)` and be perfectly
honest about it, and the suite above it still prints PASSED. If the reason is
one that is *always* true — "the disk isn't mounted yet", when the code runs
before mounting — then that test has never run once, in the whole life of the
project, and nothing anywhere says so. §650 found six of those by reading code.
This entry is the machine that finds the seventh. It works by watching: every
boot now writes down which tests said SKIP, and if a test has said SKIP on
every one of the last ten-or-more boots, the build stops and names it.

### Why watching rather than reading

§650 already recorded the choice of a dynamic check over a static one, and the
reason has not changed: answering "is this function reachable only from a
pre-mount call site?" needs cross-module call-graph resolution over a
100k-line file, which is the kind of gate that becomes unreliable and then
gets ignored. What this entry adds is what a *dynamic* check has to get right,
and the answers are not obvious in either direction.

### The four decisions

**1. The unit of evidence is a boot, and rows that cannot testify are excluded
rather than counted as "did not skip".** Three exclusions: a row written before
the `skips` field existed, a boot that did not reach `BOOT_OK`, and an
`experiment` probe. Each is the same argument — a row that says nothing must
not be read as saying "no". Counting a pre-field row as "this skip did not
fire" would break the 100% streak of every genuine offender, i.e. fail in the
direction that hides the bug. The cost is that the gate is silent for the
first ten boots after landing, which is the price of not being wrong.

*Alternative rejected:* treat a missing field as "no skips". Cheaper, and it
would have made the gate live immediately — on a dataset where every historical
row asserts, falsely, that nothing was ever skipped.

**2. The threshold is 100% of N, N ≥ 10 — not a percentage.** A 90% rule would
catch a skip that fires almost always, which sounds strictly better and is
worse: "almost always" is exactly what a genuine host-capability skip looks
like when the harness is occasionally run with different flags, so a 90% rule
converts real preconditions into recurring false positives. 100% is the only
threshold that means *nobody has ever observed the precondition holding*,
which is the claim being made. The floor of 10 is a floor on confidence: two
consecutive skips are not evidence, and a gate that is wrong most of the time
is disabled within a week.

**3. The window is bounded at 25 boots, not "all of history".** A skip fixed
today would otherwise keep its 100% rate for as many boots as it had
accumulated before the fix, and the gate would go on failing long after the
defect was gone. That is how a gate teaches its reader to reach for
`--no-verify`.

**4. The allowlist fails in both directions.** Five skips fire on every boot on
this host and are not defects — a CPU without PCID, a single-core QEMU. From
the log they are *indistinguishable* from §650's six, which is the whole
difficulty, so the allowlist is where that distinction is written down rather
than inferred. Two rules keep it from becoming a dumping ground:

- Every entry must state the **observable condition** that would stop the skip
  firing ("boot with `-smp 2`"). An entry that cannot say what would change in
  the world does not describe a precondition; it describes a defect someone
  wanted to stop hearing about. A test asserts this mechanically, weakly, by
  requiring a justification of some length — it cannot check the sentence is
  true, only that someone wrote one.
- An allowlisted skip that has **stopped** firing is a hard failure. Either its
  precondition now sometimes holds, making the entry a false statement about
  the current tree, or the section was renamed or deleted and the entry names
  nothing. Both are fixed by deleting a line, so the failure is cheap; a stale
  allowlist entry that nobody is forced to look at is not.

### A skip is not the same thing as a section that did not run

This is the decision the first version got wrong, and it cost three of its four
findings.

`ipc::io_ring` calls its two file-handle cases from `self_test()` before `/tmp`
is mounted — where they skip — and again from `self_test_fh()` after the mount,
where they pass. The pre-mount call is not an oversight; it is a **deliberate
tripwire**, and the comment above it (`kernel/src/ipc/io_ring.rs:1290`) says so:

> They are still reported, because "expected" and "invisible" are different
> things: `self_test_fh` below re-runs them once /tmp is mounted, and if *that*
> call ever stopped happening the only evidence would be these two lines never
> being followed by an OK.

`[mm] Zero-on-free` is a third case of the same shape. So a gate that counts
every SKIP line would have produced three **permanent** false positives, and
the fix its own message invites — move or delete the pre-mount call — would
have destroyed the tripwire. A permanent false positive is worse than no check;
`scripts/test-ki-dupes.py`'s docstring already says this about a different gate,
and this is the second time the project has paid for learning it.

**The decision: the recorder splits each boot's skips in two.**
`partition_skips` in `boot-history.py` returns `uncovered` — the section
announced SKIP and no other line under that tag reports it as having run — and
`covered`, everything else. Only `uncovered` reaches the gate; both are stored,
because "it skipped here and ran there" is a fact worth keeping.

This is not a suppression list, which is the point. "The only evidence would be
these two lines never being followed by an OK" is *literally* the condition
`partition_skips` computes, so the day `self_test_fh` stops being called the
pair moves from `covered` to `uncovered` and the gate begins accumulating
against it unprompted. The tripwire acquired a bell it did not have before.

*Alternative rejected:* allowlist the three. It reaches the same green build
today and throws away the tripwire's whole purpose — an allowlisted skip is
excused permanently, including on the day it becomes real.

**Which way the matcher is allowed to be wrong.** A section counts as having
run if some line carries the same tag, contains the section's key text, and
mentions skipping in no spelling. All three clauses are scar tissue: the kernel
spells a section's parentheses differently between its skip and its result
(`Positioned I/O (pread/pwrite)` vs `Positioned I/O (pread/pwrite preserve the
cursor): OK`), so only the text before the first `(` is common to both — with a
length floor, or a key like `RX` matches half a driver's output; and two suites
narrate the skip in *prose* on the line above the machine-readable one
(`[hotplug]   Single-CPU: skipping offline/online cycle`), which a naive "not a
SKIP line" test reads as proof the section ran. Where it errs it errs toward
`uncovered`: a wrong `uncovered` becomes a visible accusation someone resolves,
a wrong `covered` excuses a section forever with nothing to see.

### The parser is the part that fails silently, so it is the part with the tests

Every one of the 32 tests in `scripts/test-check-boot-skips.py` exists because
a parser bug here is invisible and permanent: a skip *name* that carries
anything run-specific is never equal to itself on the next boot, so the
100%-of-N count can never accumulate and the gate reports "all clear" forever.
That is a worse outcome than having no gate, because it also reports a number.

The first draft did exactly this. `_SKIP_RE` used `\s*` between the log tag and
the word SKIP; `\s` matches a newline, so it paired a tag on one line with a
SKIP hundreds of lines later and produced names like `'[mm] Frame allocator
self-test PASSED — 2 section(s) [mm] Kernel heap allocator initialized'`. Five
of the nine names in the first real run were garbage of that shape, and every
one of them looked like a plausible log line. Three consequences, all now
pinned by tests:

- Every quantifier in the regex is `[ \t]` or `[^\n]`, never `\s` or `.`.
- The reason is stripped by scanning **inward from the closing paren**, because
  reasons nest their own: `Zeroed frame allocation (HHDM is not mapped yet
  (running before page_table::init))` splits wrong at the last `" ("` and right
  at the first only by luck.
- The closing summary `— 2 section(s) SKIPPED` and the ledger-overflow line
  `SKIP: 3 further section(s)` are excluded: both carry a *count*, so a name
  derived from either differs between boots — the same silent-forever failure
  arriving through the front door.

`SKIPPED` is matched as well as `SKIP:` because the kernel says both, at ~40
sites. Normalising in the parser rather than renaming the call sites: a rename
to satisfy a parser is the tail wagging the dog, and the parser is where a
reader looks when a name looks wrong anyway.

### What it found immediately: four claims, one instance

Pointed at one green log the first version named four sections. Checked against
the source and against the rest of the same log, **one** was real:

| Claim | Verdict |
|---|---|
| `[io_ring] File handle read/write` | ran 1045 lines later — the tripwire above |
| `[io_ring] Positioned I/O (pread/pwrite)` | ran, same reason |
| `[mm] Zero-on-free` | ran 46,883 lines later |
| `[mm] Zeroed frame allocation` | **genuine**: `mm::frame::self_test` runs before `page_table::init` on every boot, so "HHDM is not mapped yet" is a constant |

That ratio is the entry's most useful content. The gate's *design* was sound and
its *evidence* was sound; what was wrong was an unexamined assumption sitting
between them — that a SKIP line is a report about the boot, when it is only a
report about one call site. Three of four is not a bad rate for a first pass;
shipping it without reading the source for each finding would have been the
error, and the near-miss is why every finding this gate produces should be
checked against the log's later lines before it is believed.

`[mm] Zeroed frame allocation` is recorded in `known-issues.md` and deliberately
**not** fixed here — it needs a post-`page_table::init` entry point that does
not exist — for the reason §650 gives: a gate that lands together with the
failures it finds is a gate whose first act is to be worked around.

One row of `bench/boot-history.jsonl` was rewritten rather than left alone: the
single row written during the hours the field held every skip. Its nine names
are exactly the partition of its own serial log, so the correction is derived
rather than guessed, and the alternative was one row whose `skips` field means
something different from every row after it — a silent mis-count later, in
exchange for a purity about append-only files that nothing depends on.

### How to reverse it

Delete `scripts/check-boot-skips.py`, its test suite, and `check_boot_skips` in
`scripts/boot-test.sh`. Leave `partition_skips` and the `skips` /
`skips_covered` fields in `boot-history.py`: they are cheap, already merged
across lanes, and the only record that exists of which sections did not run on a
given boot. Reversing costs the answer to "has this ever run?", which was
unanswerable before today and is the question §650's six spent their whole lives
evading.
