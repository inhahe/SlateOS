## 971. A version-history entry holds the file as it stands after the save that made it

**Date:** 2026-09-27 · **Decided by:** Operator (Claude recommended this option) · **Lane:** A

Answering A-Q14, option A. The operator's answer was given in lane C's chat on
2026-09-27 and relayed by lane C; the verbatim record is
`requests/c-abf-the-operator-answered-a-q14-a-q15-b-q8-f-q1-f-q2.md`.

**In short:** in a directory that keeps previous versions of its files, each
stored version is now the file as it stands **right after** the save that made
it, not the content that save replaced. This is what lets the slow part of
keeping a version -- reading the file back and checksumming it -- happen after
the save has returned, as §936 decided: once a save has returned, the content
it replaced is gone. Going back works as before: after three saves you can
recover the file as it was at saves 1 and 2, and save 3 is the file itself.

**The question it settled.** §936 (A-Q10) said both "every save reads back the
old contents" and "the read-back happens after the write has returned". Those
two halves cannot both hold, because after the write returns the old contents
no longer exist. The options were:
- **A** (chosen): store the post-save content, holding nothing extra in memory.
- **B**: copy the old content into memory during the save and checksum it
  afterwards. The stored versions stay exactly as they were, at the cost of a
  second in-memory copy of a large file while it is being saved.

**What it obliges.**
- `kernel/src/fs/history.rs` `try_auto_record` moves after the write, and
  records the content the write produced.
- Test 7 in that file's `self_test` asserts that after writing v2 the history
  holds v1. Its meaning changes deliberately: after writing v2, the newest
  entry is v2, and v1 is the entry the previous save made. It is rewritten to
  say so, not deleted.
- The cost §936 already accepted stays: a crash between the save returning and
  the entry being written loses that one entry. It is still bounded to one
  version of one file, in a directory that opted in.

**Revisit if** a user needs the content a save *replaced* even though no earlier
save recorded it: for example, the first save into a directory just enrolled.
That is B's one advantage, and it is available again as an opt-in mode if it is
ever wanted.
