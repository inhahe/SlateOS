## 1560. A file's tags are its `user.xdg.tags` attribute, and nothing in the kernel indexes them

**Date:** 2026-10-09 · **Decided by:** Claude (operator-approved scope: §978's
doors for the per-file tables, built as §1533 built `fcomment`'s) · **Lane:** A

**In short:** a person can label files with tags ("work", "taxes 2026") and
find them by those labels. The tags were already stored on each file, as an
extended attribute (a small named value the filesystem keeps with a file)
called `user.tags`; but the kernel also kept an index in memory, mapping each
tag to the *names* of its files. Renaming a file dropped it out of every
search until someone rebuilt the index by hand, a reboot emptied the index,
and `/proc/tags` showed every tag in use to any program, whether or not it
could read the files. Now a file's tags are its `user.xdg.tags` attribute --
the name KDE's file manager and search already use -- and searching walks the
folders and reads it. Programs reach tags with the attribute calls they
already have; that is the door §978 asked for.

| | For | Against |
|---|---|---|
| **The file's own `user.xdg.tags` attribute, searched by walking (chosen)** | the tags follow the file through every name and rename, on disk on ext4, with no bookkeeping; what KDE's Dolphin and Baloo read and write, and what `cp -a`, `tar --xattrs` and `rsync -X` carry; `getxattr` is the door, with Linux's permission rules; `design.txt`: "user-facing tagging is better done at the GUI/search-index level, not in filesystem metadata" | a search reads every file under its root, bounded at 65536 entries, and says when it could not see everything; no list of every tag in use, so the kernel's sidebar model shows no Tags section |
| Keep the index, re-keyed by file identity (as the ACLs were, §1527) | a search is a lookup | still in memory: empty at every boot until something rebuilds it, so still wrong in between; a second copy of what the filesystem keeps, kept in step by hand on every rename, link and unlink; and an index of everyone's files is a search service's job, which in a microkernel belongs in userspace |
| Keep `user.tags` | nothing to change | a name only this kernel used: tags set by KDE (or any freedesktop.org tool) would not be seen, and ours would not be seen by them |

**What changed with it:**
- `fs::tags` reads and writes `user.xdg.tags` through the VFS's attribute
  calls, as the caller: `get`, `set`, `add`, `remove`, `clear`; and `search`,
  `search_multi` and `list_tags` walk a subtree (not `/proc`, `/sys` or `/dev`)
  and report `incomplete` when they stopped short.
- What may be a tag is wider, for the same reason as the name: text up to 64
  bytes with no comma, no control character and no space at either end. Spaces
  inside, `/` (Baloo nests tags with it) and letters beyond ASCII were refused
  before, and are what Dolphin lets a person type.
- `/proc/tags` counts operations only.
- `fs::sidebar`'s Tags section is empty: its entries came from the index, and
  each named a `/tags/<tag>` path that nothing served.
- kshell's `tag index` is gone; `tag list` takes a path to walk.
- `open-questions.md` A-Q22 no longer asks about tags. It said renaming a
  tagged file would keep or lose its tags depending on the answer; the tags
  were always the file's attribute, so only the index ever went by name, and
  with the index gone they follow the file whatever A-Q22's answer is.

**Not done:** nothing to carry over -- no file was ever given a `user.tags`
attribute outside this module's self-test, which ran on `/tmp`, in memory.
`queryable`, the last of §978's per-file tables, went the same way the same day (§1561).
