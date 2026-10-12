## 1561. A file's queryable attributes are `user.slate.<name>` attributes, each value marked with its type

**Date:** 2026-10-09 · **Decided by:** Claude (operator-approved scope: §978's
doors for the per-file tables; the last of them, after §1533 and §1560) ·
**Lane:** A

**In short:** BeOS let a person find any file by facts about it -- every song
by one artist, every photo wider than 4000 pixels -- because the filesystem
kept those facts, with their types, on each file. SlateOS had the start of
that, but kept the facts in a table in the kernel's memory: they vanished at
every reboot, no program could reach them, and whoever asked could set them on
any file without permission. Now each fact is an extended attribute (a small
named value the filesystem keeps with a file) named `user.slate.<name>`, its
value starting with a two-character type mark -- `t:` text, `i:` a whole
number, `b:` true or false, `x:` raw bytes -- so a song's artist reads
`t:The Beatles` and its bitrate `i:320`. Programs reach them with the
attribute calls they already have; that is the door §978 asked for.

| | For | Against |
|---|---|---|
| **One `user.slate.<name>` attribute per fact, the type a marker at the front of the value (chosen)** | on disk on ext4, so a reboot keeps it; follows the file through every name, rename and copy (`cp -a`, `tar --xattrs`, `rsync -X`); the attribute calls are the door, with Linux's permission rules; readable and writable with `getfattr`/`setfattr`; a value another tool wrote without a marker still reads, as text or bytes | a query reads the attributes of every file under its root (bounded at 65536 entries, saying when it stopped short) where an index would look up; ext4 keeps all of one file's attributes in a single block, so a few KiB of them per file |
| The type as a binary code before the value (Haiku's emulation on Linux: `user.haiku.<name>`, a four-byte type, the data) | exact: any type, nothing to parse | `getfattr` shows bytes, and a person cannot set one with `setfattr`; the only tools that read it are Haiku's own build tools |
| The type in the name (`user.slate.int.Audio:Bitrate`) | the value is plain | one fact can then exist twice, once per type, and a query must look for every spelling |
| Keep the table (identity-keyed since §957), add query system calls | a query is a scan of memory | gone at every reboot, so useless for anything a person would want to search later; a second copy of what a filesystem can keep; new calls where `getxattr` already works |

**What changed with it:**
- `fs::queryable` sets, reads, lists, removes and clears attributes through
  the VFS's attribute calls, as the caller -- which is the first permission
  check it has ever had -- and `query` walks a subtree (not `/proc`, `/sys` or
  `/dev`), comparing as before (text with case ignored, integer ranges,
  substrings, all-of or any-of), and reports `incomplete` when it stopped
  short.
- The in-memory store, its value indexes and its registered schemas are gone,
  and so is its place in `fs::perfile`'s lifecycle tables: an attribute on the
  file needs no hook to follow a rename or to end with the file. The schemas
  are a fixed list of well-known names now (`queryable::WELL_KNOWN`), as
  advice.
- `/proc/queryable` counts operations and shows the well-known names.
- kshell's `qattr index` and `qattr schema init` are gone; `qattr names`
  shows the well-known names; `qattr query` takes a root, `/` by default.

**Not done:** nothing to carry over -- the table emptied at every boot, and
only the kernel shell and this module's self-test ever set anything in it.
An index for fast queries, when one is wanted, is a userspace search
service's to keep; `design.txt` puts such indexing "at the GUI/search-index
level". With this, every per-file table §978 named has its door.
