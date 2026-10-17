## TD-A-MEMFS-PAGES-ARE-KEPT-TWICE (lane A, 2026-10-17) — OPEN

**Status:** OPEN -- fixed on lane-a-wip 2026-10-17 (design-decisions
§1574), awaiting a boot on main. memfs reads skip the page cache, and a
mapped memfs file keeps its pages in frames of its own, which the cache
adopts rather than copies. This differs from the fix proposed below: §1574
says why the cache did not become memfs's store.

**Mutation proofs still owed (cut off by the PC move, 2026-10-17):** a
boot showed memfs's frames test failing when `FileFrames::frame` hands a
page a new frame each time ("a page was given two frames"). Still to
boot, each expected to fail its test:
- `handle::cached_page`'s adopt closure answering `None` -- page_cache's
  "a mapping's page of a memfs file is a copy, not the file's own frame";
- `get_or_fill_from` ignoring what `adopt` offers -- "a filesystem's own
  frame was copied, or adopted twice";
- memfs's `caches_contents` answering true -- "a read of a memfs file left
  a copy in the page cache".

**In short:** a file on a memory filesystem (memfs: `/tmp`, and every memfd
since design-decisions §1573) keeps its bytes in memfs's own buffer. The
page cache also keeps a copy of each 16 KiB page that is read or mapped:
`read(2)` goes through the cache (§38), and so does a mapping (§1572). So a
memfs file that is read, or mapped, takes its memory twice. A program that
maps a 64 MiB memfd shared, as a compositor's client does with its buffers,
uses 128 MiB.

**Where:**
- `kernel/src/fs/memfs.rs`: `MemFsNodeKind::File(Vec<u8>)`.
- `kernel/src/fs/vfs.rs`: `read_at_routed` and `object_read`, which fill the
  cache from the filesystem for every file with a stable identity.
- `kernel/src/mm/page_cache.rs`: the fills.

**The proper fix:** memfs keeps a file's data in the page cache itself, as
Linux's tmpfs does (`shmem`): the cache's pages are the only copy.
- Such a file's cache state is marked as having no backing store. Its pages
  are then never evicted or dropped, need no writer and are never written
  back; a cut drops the pages past the end.
- memfs's reads and writes go to those pages.
- Its size limit (`A-MEMFS-HAS-NO-SIZE-LIMIT`) counts them.

It is independent of the memfd change and best done once, for every memfs
file.

**Until then:** correct, only wasteful. A memfs file read once costs its
size again in the cache. The cache's clean pages are dropped under memory
pressure (`page_cache::shrink`), so read-only use gives the memory back.
Mapped pages stay as long as they are mapped.
