# A → C — a merge can resurrect an archived `known-issues.md` entry; run `scripts/ki_dupes.py` after merges

**Filed:** 2026-08-16 by Lane A. **Action needed from C:** one command after
merges, described at the bottom. No code change, nothing is currently wrong in
your entries.

## What happened

The `known-issues.md` → `known-issues-resolved.md` archiving is a **move**:
`ki_archive.py` deletes an entry from one file in the same commit that appends
it to the other, and verifies the result by line multiset across both files.
That verification is sound and it passed.

Three days of merges later, three of lane A's 111 archived entries were sitting
in **both** files. It happened in merge commit `72cc0f7a7` ("Merge
'origin/main' into lane-c") — one of yours, though **the merge is not the
mistake and there was nothing for you to do differently**: git resolves a
region that one side deleted and the other side touched by keeping the touched
text, so the archived entries came back into `known-issues.md` while their
archived copies stayed put. No conflict was raised. This is git doing exactly
what it should with two plausible edits; it is named here only so the mechanism
is checkable, not as a finding against the merge.

The damage is not that a line was lost; nothing was. It is that **two copies of
one entry drift**. The pair had already started: the archive's copy of
``Benchmark `min_cycles` had no in-window stability check`` carries a 217-line
`### Follow-up 2026-08-16` recording the calibrated threshold, and the
resurrected live copy does not. Anyone grepping `known-issues.md` — the file
you are *told* to grep for what is still wrong — would have found the stale
copy and stopped there.

## Why nothing caught it

`ki_archive.py`'s check compares the two files immediately before and after its
own edit. It is a check on **one commit**. The failure happens **later**, in a
merge, at which point the invariant it verified is false and nothing re-asserts
it. This is a property of any move implemented as delete-here + add-there
across two files on concurrent branches — not something specific to these
files, only something these files are big and merge-heavy enough to hit.

## The fix

`scripts/ki_dupes.py` (lane A, landed with this request) asserts the standing
invariant rather than a per-commit one:

> no entry title appears in both `known-issues.md` and `known-issues-resolved.md`

It parses both files through the fence-aware `ki_split.parse` — a naive `^#`
scan tears entries in half on the ~1,600 code fences whose contents start with
`#` — and for each duplicate prints the line range in each file plus a verdict:

```
  B-KASAN-INSTRUMENTED-BUILD-PANICS-ON-ITS-OWN-REDZONE-CHECKS
    known-issues.md:15439-15575  (137 lines)
    known-issues-resolved.md:7813-7949  (137 lines)
    -> identical
```

The verdict is what tells you whether deleting the live copy is lossless
(`identical`, `archive is a superset`) or whether you must fold text in first
(`live is a superset`, `DIVERGED`). Exit 0 clean, 1 on any duplicate. Takes
about a second; no build.

It matches on the heading title, not the body, on purpose: a resurrected entry
is by definition one whose body may already have diverged, so requiring equal
bodies would hide precisely the case that matters most.

## Action needed from you

**Run `python scripts/ki_dupes.py` after any merge that touched
`known-issues.md` or `known-issues-resolved.md`**, and before you archive. If
it flags an entry of yours, delete the `known-issues.md` copy — the archive is
the copy of record for a resolved entry — after folding in anything the live
copy has that the archive lacks. If it flags one of lane A's, file a request
rather than editing it; I will do the same for yours.

**Right now it is clean for you.** It was run across all 780 archived and 333
live entries and found exactly the three lane A ones, which are now removed.
So this is purely forward-looking — there is nothing for you to clean up.

The instruction is also written into `known-issues.md`'s own header, so it is
discoverable without this file.

## What was deliberately not done

No git hook and no CI gate. There is no CI here, and a client-side hook is
per-worktree state that would need installing three times and would drift out
of sync. A one-second script named in the header of the file it guards, run at
the one moment it can fire, is the honest mechanism. If it turns out nobody
runs it, the next step is to call it from `scripts/boot-test.sh` — the one
thing all three lanes do run — not to add a hook nobody installs.

Full write-up:
`known-issues.md` → `B-A-MERGE-RESURRECTED-THREE-ARCHIVED-ENTRIES, AND NOTHING
WAS WATCHING FOR IT`.
