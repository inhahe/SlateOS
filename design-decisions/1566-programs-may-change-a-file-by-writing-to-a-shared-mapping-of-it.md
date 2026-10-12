## 1566. Programs may change a file by writing to a shared mapping of it

**Date:** 2026-10-09 · **Decided by:** Operator (Claude recommended this option) · **Lane:** A

Answering A-Q24, option A. The operator's answer, verbatim, is in
`operator-answers/2026-10-09-open-questions-answers.txt` ("A-Q24: A"). It
reverses the operator's June decision to refuse it (§22, narrowed by §23:
"won't fix"), now that software arriving on the system needs it.

**In short:** a program can ask for a file to appear in its memory. Until now
it could only read the file that way, or keep its changes to itself; writing
the memory so that the file changed, and every other program with the file
mapped saw the change, was refused. That is now to be built: SQLite's faster
journal mode (WAL), the LMDB database, and any program that edits a file
through a mapping need it.

**The alternatives not taken:** only for files kept in memory (`/tmp`), which
helps neither SQLite nor LMDB, whose shared files sit beside the database;
keeping the refusal.

**What it obliges** (the largest piece of memory-management work left):
- one copy of each page of a file in memory -- the page cache §23 built for
  reading -- that a writable shared mapping maps directly, so every mapping of
  the file and every `read` see the same bytes;
- marking a page changed when a mapping writes it (the page-table dirty bit),
  and writing changed pages back: on `msync`, on unmapping, on `fsync`, and in
  the background;
- a `write` to the file and a write through any mapping being the same change,
  through `truncate` (pages past the new end stop being mappable, as Linux's
  `SIGBUS`) and through a crash (what reached the disk is what `msync`/`fsync`
  promised);
- the same for the native `SYS_MMAP_FILE` (lane D's
  `requests/d-a-a-native-program-cannot-map-a-file.md`) and the Linux ABI's
  `mmap`, where a writable `MAP_SHARED` of a file answers `ENOSYS` today;
- `known-issues/TD22.md`, which records the refusal, closes when it is done.
