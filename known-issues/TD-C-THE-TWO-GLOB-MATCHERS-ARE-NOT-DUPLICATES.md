## TD-C-THE-TWO-GLOB-MATCHERS-ARE-NOT-DUPLICATES -- 2026-09-17

**In short:** the tree has a shared glob-matching crate and a second matcher
inside the backup program. That looks like exactly the duplication worth
removing, and it is not: they answer different questions and unifying them
would break the backup program's exclude patterns. Written down so the next
person who notices the pair does not have to work that out again -- or worse,
does not.

**Date:** 2026-09-17. **Lane:** C.

| | |
|---|---|
| `apps/globmatch/src/lib.rs` (511 lines) | used by `apps/filesearch` and `apps/indexer` |
| `apps/backup/src/main.rs:100-330` (231 lines) | used by `is_excluded` |

**Both implement `*`, `?` and `[...]`.** That is the whole of the resemblance
and it is why the pair looks like an oversight.

**They differ on the one thing that matters for their callers: whether a
pattern knows about path separators.**

* `apps/backup` is **path-aware**. `?` is written `Some(&b'?') if t != b'/'`,
  so it will not match a separator, and `**` is recognised as a whole-segment
  wildcard that crosses them. That is what an exclude line like `cache/**`
  requires, and an exclude pattern that silently matched across directories
  would leave files out of a backup.
* `apps/globmatch` is **flat**. `?` is `Some('?') => true` -- any character,
  separator included. That is right for matching a search term against a
  filename, which is what its two callers do.

**So this is the compositor's rectangle again**, from
`TD-C-TEN-RECTANGLE-TYPES-IN-THREE-SPELLINGS`: two types with the same surface,
different semantics, each correct where it sits. The tell in both cases was not
the API -- it was one line of the implementation. Reading the signatures would
have made them look identical.

**Do not unify them.** If a third caller needs globbing, the question to ask
first is whether it matches *names* or *paths*, and the answer picks the
matcher. A wallpaper exclusion filter, for instance, matches filenames within
one folder, so it wants the flat one.

**One real obstacle, recorded for whoever needs the flat matcher from the
shell:** `apps/globmatch` is under `apps/`, and 136 crates under `apps/` depend
on `gui/` while none go the other way. Reaching it from `gui/desktop` needs the
same relocation `gui/pathcodec` got, and that is a deliberate move rather than
something to do as a side effect of wanting a filter.
