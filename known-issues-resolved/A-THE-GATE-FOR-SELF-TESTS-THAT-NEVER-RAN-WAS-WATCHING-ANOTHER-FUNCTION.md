## A-THE-GATE-FOR-SELF-TESTS-THAT-NEVER-RAN-WAS-WATCHING-ANOTHER-FUNCTION (lane A, 2026-09-12) — FIXED

`check-gated-selftests.py` exists to fail the build when a self-test behind an
`if` has never once announced itself. For the FAT site it reported *ran* on 135
of 136 boots. The suite has never run at all.

**The marker named a different function.** Each gated dispatch in `main.rs`
carries a `RAN-IF:` comment giving the serial line that proves it executed. The
one on `fs::fat::self_test()` (main.rs:1576) named `[fat] Running mkfs/format
self-test...`, which is printed by `format_self_test` (fat.rs:6029) -- a
*different* suite, dispatched unconditionally three hundred lines below. Its
banner is therefore on every boot, so the marker was permanently green and
nothing could ever have turned it red.

**What was actually behind the gate:** `fat::self_test` is 1,184 lines covering
read, write, create, delete, mkdir, rmdir and directory listing against a live
FAT volume. Not a stub, and not redundant with `format_self_test`, which formats
a RAM disk. That coverage has never executed in the harness.

**Two witnesses, and they agreed for the same reason.** The allowlist comment
recorded that on 2026-08-31 all six gated sites `were audited ... against a full
serial log and every one of them was found to run on this host`. That audit read
each site's *declared* marker -- the same mislabelled string this gate reads. An
audit derived from the annotation can only confirm the annotation. Nothing in
either check touched the one fact that would have settled it: whether the
declared line is printed by the function it is attached to.

**Found by asking a question the fix could not answer for itself.** The prompt
was a stale-looking comment, not a failure. The check that settled it was written
from the *convention* (a marker must be emitted by its own call) and run against
all six sites, so it could report the other five as correct -- which it did. Had
it been derived from the FAT bug it could only have rediscovered the FAT bug.

**Independent confirmation before acting**, because the claim `this never runs`
otherwise rested on the same marker list being impeached: `[fat] Running
self-test` occurs 0 times in the retained serial logs while the mkfs banner
occurs once, and `fat_ok` is `fs::fat::init("vda")`, which cannot
succeed on a harness that mounts an in-memory root and attaches vda as a raw swap
disk. The code comment at main.rs:1561 has said so in prose the whole time. The
prose was right and the machine-checked claim was wrong, which is the wrong way
round -- the machine-checked one is the one people trust.

**Fixed in two steps, and the gate chose the order.** The plan was one atomic
commit -- correcting the marker alone turns a false green into a hard failure
ten boots later (`DEFAULT_MIN`), which would block all three lanes. Applying it
to a scratch copy of the tree first showed that the second half cannot land yet:
`live` is the newest boot`s `gated_ran` keys, so a marker just declared in source
is not live, and allowlisting it fails as `names nothing`. That refusal is right
-- an allowlist that accepts markers nobody has ever observed is a place to hide
phantoms -- so the entry waits for a boot instead of the gate being loosened to
accept it. Step 1 (this commit): the annotation names the banner its own call
prints, and the 2026-08-31 audit note is corrected in place, since a wrong
finding that has been *checked* is harder to dislodge than an unexamined one.
Step 2, after the next boot records the corrected marker: the allowlist entry,
with the condition that would end it -- a FAT-formatted vda. Nine boots of
margin, and step 1 leaves both gates green (verified against real history).

**Still true after the fix:** the suite still does not run. The gate now says so
honestly instead of claiming the opposite. Making it run means giving the harness
a FAT volume, which is a disk-layout change to a boot test three lanes share, and
is deliberately not bundled here.

**And the new gate had the same defect, which its own tree caught.** `check-ran-if.py`
tested `"--self-test" in argv`, so `--selftest` fell through to the real scan and
exited 0 -- the command asking whether the checker is still correct answering
yes without asking. `check-selftest-flag-spellings.py` refused the push over it,
naming sixteen scripts that had already had this shape. The fix is
`selftestflag.wants_selftest(argv)` plus `unknown_options`, so an unrecognised
flag is an error rather than a fall-through. Recorded because it is the same
defect as the entry above, one level up, written by someone who had spent the
hour thinking about nothing else: success and not-having-run must not be the
same observation.

**A guard for this already existed, and it passed.** `check-self-tests-wired.py` has
validated `RAN-IF` markers since before this bug, and
`test_marker_must_live_in_the_tested_file` states the intent exactly:
*another module's line does not vouch for this one*. It did not fire, and the
reason is one word wide -- **file**:

```python
homes = {defs[s][0] for s in syms if s in defs}      # defs[s][0] is the FILE
if not any(lit in files.get(rel, "") for rel in homes):
```

`fs::fat::self_test`'s home is `fs/fat.rs`, and `format_self_test` prints the
declared banner *from that same file*, so the literal was found and the marker
accepted. The guard was correct about a different module and blind to a
different function in the same one.

This matters for whoever reads the two gates later and sees overlap.
`scripts/check-ran-if.py` is not a duplicate: it resolves the annotated call to
its `fn` and checks that body, which is the granularity this check is missing.
Deleting either one restores the gap -- the file-level check runs on every
marker including ones whose callee cannot be resolved, and the function-level
one is what distinguishes neighbours sharing a file.
**Follow-up: the gate that caught this was itself nearly vacuous.**

`check-ran-if.py` shipped in 0417ad5b8 resolving the annotated call by its
**last path segment only**. There are **719** definitions of `fn self_test`
under `kernel/src`, so `fs::fat::self_test` was checked against all of them and
passed if any one printed the declared marker. It caught the bug above purely
by luck: that marker belongs to `format_self_test`, a differently *named*
function. Had a neighbouring module's `self_test` printed it, the gate would
have reported OK having verified a body the annotation never named -- the same
defect it exists to catch, one level up. The commit message claimed it
`resolves each annotated call to its fn`, which was true of the design and
false of the code.

Two hardenings, both verified against real history rather than fixtures --
`main.rs` restored from `0744cd8a0` must still yield exactly one finding, and
the current tree none:

- The module path now picks the file: `fs::fat::self_test` is satisfied by
  `fs/fat.rs` or `fs/fat/mod.rs` and nothing else. (`os.path.relpath` returns
  backslashes on Windows, where this runs, so the comparison is normalised --
  without that it would report six findings on a clean tree.)
- It scans every `.rs`, not just `main.rs`. All six annotations live in
  `main.rs` today and the sibling gate assumes the same, which made the
  assumption consistent but unenforced; an annotation added elsewhere would
  have been silently unchecked. `scripts/hooks/pre-push` carries the identical
  lesson -- its gate 20 selector excluded every shell script for months and an
  excluded file looks exactly like one with nothing to find.

A regression case covers the namesake hole directly: two modules defining the
same function, only the wrong one printing the marker, must be a finding.
Kept because without it the next refactor can quietly widen the resolver again
and every symptom would look like a pass.
**The structural gap is real, and is the next commit.** A `RAN-IF` is a comment;
nothing verifies that the line it names is printed by the function it annotates.
A static check does: resolve the annotated call to its `fn`, assert the literal
appears in that body. It never reads a serial log, so it cannot be satisfied by
the evidence that satisfied both this gate and the 2026-08-31 audit. Run against
the tree *before* the correction it reported exactly one finding -- this one --
and five clean, which is the discriminator: a check derived from the fix could
only have reported the fix.
