## TD-EXPLORER-UNREADABLE-RECYCLE-ENTRY -- **FIXED 2026-09-07**

**Fixed 2026-09-07 (lane C).** A damaged entry is listed instead of skipped.
`RecycleEntry::original_path` and `recycled_at` are `Option`, `None` meaning
"this entry's metadata would not parse" -- the one thing genuinely unknown is
marked unknown, rather than the whole row being hidden because part of it is.

What a caller may do follows from the field and needed no second flag:
restoring wants somewhere to restore *to*, so it is refused with a reason;
deleting wants only the id, so emptying the bin now reclaims the space.

Three details that are not incidental:

- **The size is still measured**, from the data on disk rather than from
  `meta.txt`, because space is usually what brings a user to the recycle bin
  at all. A row that said "unknown item, unknown size" would answer none of
  the question they came with.
- **`display_name` never falls back to the id.** The id is a hash; shown in a
  name column it would read as though it were the file's name.
- **`purge_old` skips it.** Its age is unknown, and unknown must not be read
  as old -- ageing it out on a guess would delete a user's file in order to
  tidy up a metadata problem.

The UI decision the original entry was waiting on turned out to be the one it
had already proposed, and small enough to take: "Unknown item (damaged entry)",
delete available, restore refused.

Six tests. Mutation-checked: restoring the old skip fails four of them.

Original entry follows.

**Status: ~~OPEN~~ FIXED 2026-08-16** (lane C). `apps/explorer/src/fileops.rs`,
`RecycleBin::list`.

The recycle-bin listing skips any entry whose `meta.txt` will not parse, rather
than failing the whole listing. That is the right trade — one corrupt metadata
file must not make every *other* recycled file unrestorable — but it has a cost
that is currently invisible: the damaged entry does not appear in the UI at
all, so the file is still on disk, still occupying space, and the user has no
way to see it or to empty it.

**The proper fix** is to surface it rather than swallow it: return the
unparseable entries alongside the good ones as a distinct variant (id and
on-disk size known, original path unknown), which the UI lists as "unknown
item" with delete available and restore greyed out. That needs a UI decision
about how such a row reads, which is why it is written down rather than done in
the sweep commit.

**What it costs while open:** a corrupt entry is undeletable through the UI and
its space is unaccounted for. It cannot cause data loss — the file itself is
untouched — and a corrupt `meta.txt` requires the disk or another process to
have damaged it, so this is rare rather than routine.
