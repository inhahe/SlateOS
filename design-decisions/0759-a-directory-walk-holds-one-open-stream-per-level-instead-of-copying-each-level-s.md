## 759. A directory walk holds one open stream per level instead of copying each level's listing, because the copy was a cap and a cap is a file quietly not visited

**Lane:** B
**Date:** 2026-09-04
**Decided by:** Claude (autonomous)

**In short:** `fts` is the machinery underneath `find`, `rm -rf`, `du` and
`chmod -R` — the thing that walks a directory tree. It used to copy out the
names in each directory into a fixed slot of 64 before going deeper, and a
directory with 65 files simply lost the 65th: no error, no message, the walk
reported success. A `rm -rf` on such a directory would say it had finished
while files were still there. It now keeps the directory open while it walks
it and asks for the next name when it needs one, so there is no number of
files that is too many. The cost is that the walk holds one open directory per
level of depth (at most 16 at once) instead of one at a time.

### What was actually decided

Three things, of which only the first is a genuine tradeoff.

**1. Hold a stream per level, rather than raising the cap.** The obvious
smaller fix is `MAX_FTS_CHILDREN: 64 -> 4096`. It was rejected because it does
not remove the failure, it moves it: the walk still loses files silently, just
in rarer directories, which is strictly worse for diagnosis — a bug that fires
on 1% of trees is a bug nobody reproduces. It also costs memory in proportion
to the new cap, in a `static` pool, whether or not any directory is that large.

*What we pay instead:* the traversal now occupies `MAX_FTS_INSTANCES *
MAX_FTS_DEPTH` = 16 of `dirent`'s 64 `Dir` slots at full depth, where before it
occupied one. That is a real coupling between two modules' capacity constants,
and it is why a `const _: () = assert!(...)` now sits beside `MAX_FTS_DEPTH`
asserting the product fits the pool: if either constant moves the wrong way,
the build fails rather than a deep walk starting to report `FTS_DNR` partway
down, which reads like a permissions problem and is not one.

**2. Match `FTS_NAME_MAX` to `dirent`'s `d_name` capacity rather than picking a
bigger number.** The component buffer was 64 bytes and a longer name was
skipped — again with no entry and no error, despite the module docs claiming an
`FTS_ERR` entry was produced. Setting it to exactly the capacity of the field
`readdir` fills makes the skip *unreachable* rather than merely unlikely, which
is a different kind of claim and the only one worth asserting. `dirent` grew a
`pub(crate) const DIRENT_NAME_MAX` for this: the number was a literal `256` in
a struct definition, and a relationship between two modules cannot be asserted
against a literal.

**3. Ownership is discharged at two call sites and nowhere else.** `DirFrame`
is `Copy` and lives in a `static` pool, so it has no `Drop` and the compiler
will not help. The pop path (`pop_and_yield_dp`) closes the frame it is
leaving, and `close_open_frames` closes whatever is left — called by
`release_instance` *before* it resets the slot, because resetting first would
null the pointers and leak every stream. `close_frame` is idempotent, which is
not tidiness: `fts_close` on a partly-walked tree runs `close_open_frames` over
frames the pop path may already have closed, and a second `closedir` on the
same pointer frees a pool slot another traversal has since been given.

### The reason the copy existed had already expired

This is the part worth remembering, because the shape recurs. The eager copy
was correct when it was written: `dirent`'s pool held 8 streams, each slot
carrying a 68 KiB inline buffer, so a walk that held a stream per level would
have exhausted it. `dirent.rs`'s own docs say so (lines 1011-1013): the small
pool "is what forced `crate::fts` to buffer children eagerly."

Then the pool went to 64 slots and nobody revisited `fts`. Worse, `fts.rs`
acquired a *second* justification for the copy in the meantime — that it "makes
a frame independent of a stream that could be invalidated underneath it" —
which is simply false: a `Dir` slurps its whole listing at `opendir` time via
`SYS_FS_GETDENTS_PINNED` and never re-reads, so holding one open is exactly as
independent of later mutation as copying its entries was.

So the workaround outlived the constraint that forced it, and then grew a
plausible-sounding reason that was never true. The general lesson: when a
constraint is lifted, the code that worked around it does not announce itself;
it sits there looking deliberate, and the comment above it drifts into fiction.
The tell here was available the whole time — two modules' docs contradicting
each other about why the same code existed.

### What is pinned by tests

Nothing about this can be exercised against a real directory from a host test:
`opendir` goes through SlateOS syscalls. So the seven tests added pin the
*facts that made the change safe*, each of which a later edit could silently
take back — the pool budget, the name-capacity relationship, that a fresh frame
owns no stream, that closing twice is safe, that a frame with no stream yields
no children, that closing empties the stack, and that `size_of::<DirFrame>()`
stays under 1 KiB. The last is a tripwire, not a budget: a frame that large is
a per-directory listing buffer wearing a frame's name, and such a buffer is a
cap.

See known-issues.md `B-FTS-DROPS-THE-65TH-CHILD-OF-ANY-DIRECTORY`.
