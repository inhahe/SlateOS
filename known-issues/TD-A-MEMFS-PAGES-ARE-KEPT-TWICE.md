## TD-A-MEMFS-PAGES-ARE-KEPT-TWICE (lane A, 2026-10-17) — OPEN

**Status:** OPEN.

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
