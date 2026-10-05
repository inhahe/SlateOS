## B-A-MERGE-RESURRECTED-THREE-ARCHIVED-ENTRIES, AND NOTHING WAS WATCHING FOR IT (lane A, 2026-08-16)

**Status: FIXED 2026-08-16** (lane A). The three duplicated copies are removed
from this file and `scripts/ki_dupes.py` now detects the class.

### What happened

Commit `6e76ce5df` archived 111 of lane A's resolved entries: deleted from
`known-issues.md`, appended to `known-issues-resolved.md`, verified by line
multiset across both files — a correct move, and the verification passed.

Merge commit `72cc0f7a7` ("Merge remote-tracking branch 'origin/main' into
lane-c") brought three of them back into `known-issues.md` without removing the
archived copies:

| Entry | live copy | archive copy |
|---|---|---|
| ``Liveness watchdog reported `SYSTEM HANG` on healthy boots…`` | 112 lines | 112 lines, identical |
| ``Benchmark `min_cycles` had no in-window stability check at all`` | 79 lines | 79 lines + a 217-line `### Follow-up 2026-08-16` the live copy never had |
| `B-KASAN-INSTRUMENTED-BUILD-PANICS-ON-ITS-OWN-REDZONE-CHECKS` | 137 lines | 137 lines, identical |

They were byte-identical at the point of the duplication and were still
identical when found, so nothing had been lost — but the `min_cycles` pair had
*already* begun to drift: the archive's copy carries the threshold-calibration
follow-up, the live one does not. A reader grepping this file for `min_cycles`
would have found the stale copy and stopped.

### Why the existing check could not catch it

`ki_archive.py`'s multiset verification is a check on **one commit** — it
compares the two files before and after its own edit. The failure happens
**later, in a merge**, and git's behaviour there is correct-by-its-own-rules:
one side deleted a region, the other side had touched it, so the touched text
survives. No conflict is raised, both files parse, and the invariant that was
verified three commits ago is now false with nothing to say so.

This is a general property of *any* move implemented as delete-here +
add-there across two files in a repo with concurrent branches. It is not
specific to these files, but these files are where it bit.

### The fix

`scripts/ki_dupes.py` — parses both files through the fence-aware
`ki_split.parse` and asserts the standing invariant, independent of any commit:

> no entry title appears in both `known-issues.md` and `known-issues-resolved.md`

It reports the line range of each copy in each file and classifies the pair as
*identical* / *archive is a superset* / *live is a superset* / *DIVERGED*, so
the reader knows whether deleting the live copy is lossless or whether text
must be folded in first. Exit 1 on any duplicate. Matching is on the heading
title, not the body, deliberately: a resurrected entry is by definition one
whose body may have diverged, so requiring equal bodies would hide the worst
case.

The file header now instructs every lane to run it after any merge touching
either file, and `requests/a-b-run-ki-dupes-after-merges.md` /
`requests/a-c-run-ki-dupes-after-merges.md` relay that to lanes B and C. There
are no lane B or lane C duplicates right now — the checker was run across all
780 archived and 333 live entries and found exactly these three.

### What was deliberately not done

**No git hook, no CI gate.** There is no CI here, and a client-side hook is
per-worktree state that would have to be installed three times and would drift.
A one-second script named in the file it guards, invoked at the one moment it
can fire (after a merge), is the honest mechanism. If this recurs despite the
instruction, the next step is to call it from `scripts/boot-test.sh` — the one
thing every lane does run — rather than to add a hook nobody installs.
