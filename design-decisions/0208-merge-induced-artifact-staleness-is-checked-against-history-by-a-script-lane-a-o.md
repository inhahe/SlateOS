## §208 — Merge-induced artifact staleness is checked against *history*, by a script lane A owns and every lane runs

**Date:** 2026-08-16. **Decided by:** Claude (autonomous).
**Lane:** A

**In short:** some compiled programs are checked into git next to a small file
recording which library they were built from. That record can be perfectly
honest and still wrong, if the library changed on a *different branch* and the
two were merged afterwards. Nothing we had could see that, because every check
we had looks at the files in front of it, and the fault is not in any one set of
files — it is in the merge. The decision is to check the **commit history**
instead, in a script that runs after every merge, and to have lane A own that
script even though the artifacts it guards belong to lane B.

### What happened, in one graph

```
                 c23cc33c0  merge origin/main into lane-b   11:11
                    /   \
   lane-b 12:22  5531f816c   2069cbd8e  lane-c: rebuild + re-stamp   13:09
   +13 libc symbols     \   /           (built against 11:11's libc.a)
                      b807390ff  main   ← both, and they disagree
```

Neither commit is an ancestor of the other. **Both were correct where they were
made** — lane C's fixtures did match lane C's `libc.a`, and lane B had no
rebuilt fixtures to invalidate. On `main` the pair is wrong by seventeen public
symbols. This is a defect class, not an incident: any tracked artifact whose
recorded inputs are owned by one lane and derived from another lane's source
has this shape, and the three-lane arrangement manufactures it.

### The alternatives

| | Where it looks | Why it was not chosen |
|---|---|---|
| **mtime** (already present) | working tree | A checkout stamps every file with one time. `create-ext4-rootfs.sh:1213` disqualifies it in its own comment. |
| **content stamp** (already present) | working tree | Asks "does the ELF match the libc it was built against?" — answer yes. Correct question, wrong question for this. |
| **build libc.a and diff** | working tree | Definitive, and needs a toolchain. The stamp check's entire design goal (`create-ext4-rootfs.sh:1219`) is to need none. |
| **record `posix/`'s tree hash in the stamp** | the artifact | Makes the stamp answer the right question *directly* — the best answer. Rejected only as lane A's call to make: it edits `services/**` and couples it to a path outside. Left open for lane B. |
| **history check** ← chosen | the commit graph | No toolchain, ~40 ms, identical answer in a fresh clone. Exactly sensitive to the merge case because it is the only one that can see a merge. |

### The two calls inside it that have a real downside

1. **The root `Cargo.toml` warns rather than fails.** Its `[profile.release]`
   genuinely can change `libc.a`, so a strict reading says it is a source. But
   in practice it far more often just gains an unrelated lane's workspace member
   — `byteread` did the same day — and **a check that cries wolf gets flagged
   into silence**, which costs more than the hole. So an opt-level change passes
   as a warning. This is a known, documented gap, and it is written into the
   script's docstring so a future reader does not "fix" it by promoting the tier
   and then watch the whole check get disabled. The rejected alternative was
   parsing the manifest to see *which* table moved: more precise, and fragile in
   a way that fails silently, which is the failure direction that matters.

2. **Lane A wrote a check about lane B's artifacts.** The ownership map says
   `services/**` and `posix/**` are lane B's, and the repair *is* lane B's — the
   nine ELFs are theirs to rebuild. But the check is read-only, writes nothing
   into either tree, and lives in `scripts/` on the `ki_dupes.py` precedent —
   lane A's own, same shape, written for a merge-only defect in a file all three
   lanes share (known-issues.md →
   `B-A-MERGE-RESURRECTED-THREE-ARCHIVED-ENTRIES`). The
   argument that settled it: **leaving detection as a request would have left
   the one check that can see this class waiting on the lane that structurally
   cannot see it.** Filing a request for a detector you can write yourself, for
   a defect the recipient's tree does not exhibit, is asking someone to act on
   evidence they do not have.

### What this commits us to

Running `python scripts/stamp-ancestry.py` after every merge, next to
`python scripts/ki_dupes.py`. Two post-merge checks is one habit, not two; if a
third arrives, they should be folded into one `scripts/after-merge.py` rather
than accumulating as a list nobody remembers.

> **Amended 2026-09-11 — this commitment is now one check, not two.**
> `scripts/stamp-ancestry.py` no longer exists. Lane B reported on 2026-08-21 that it
> could no longer succeed — their §355 stopped committing the ring-3 fixtures, so
> `git ls-files '*.stamp'` returns nothing, the script's "refuse to report clean for a
> family I cannot see" branch fired unconditionally, and every boot printed a staleness
> warning that was not true. Lane A retired it the next day: **§277**, *"A staleness
> check that can only fail is retired, not repaired"*, with the passing-path warning
> re-pointed at `ctest-fixtures.py sysroot-check`.
>
> So the post-merge habit is **`python scripts/ki_dupes.py`** alone. Left here rather
> than rewritten because the commitment was true when written and the reasoning above
> it — why lane A wrote a read-only check about lane B's artifacts — still stands.
>
> **How this was found is the part worth keeping.** I ran `ki_dupes.py` after every
> merge across a long session and never once ran `stamp-ancestry.py`. That looked like
> a lapsed habit and was a dead instruction: the script had been gone for three weeks.
> A document that says "what this commits us to" is read as present tense, and this one
> recorded its own retraction two sections away under a different number. When a
> decision is superseded, the superseding entry is not enough — the superseded
> *instruction* has to be annotated where someone will be standing when they read it.
>
> **And the habit set is one check, not two.** `check-generated-tables.py`'s own
> docstring says *"Run this after every merge, next to `scripts/ki_dupes.py`"*, which
> reads like a second habit. It is not: that script is wired into `boot-test.sh` and
> `pre-boot.py`, so the build already runs it and the sentence is telling a human to
> do what the build does. Its docstring predates its wiring.
>
> So `python scripts/ki_dupes.py` is the whole post-merge habit. That also answers
> the fold-into-`after-merge.py` trigger above: it fires when a *third* arrives, and
> there is not yet a second. The family table is data, so a new
artifact family costs four lines — and the reason it is a table at all is
`services/.gitignore`: a rule replicated per directory is a rule the next
directory opts out of by not having it.
