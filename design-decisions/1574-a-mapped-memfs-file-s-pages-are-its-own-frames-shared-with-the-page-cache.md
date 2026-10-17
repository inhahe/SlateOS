## 1574. A mapped memfs file's pages are its own frames, shared with the page cache

**Date:** 2026-10-17 · **Decided by:** Claude (autonomous; the fix
`known-issues/TD-A-MEMFS-PAGES-ARE-KEPT-TWICE` names) · **Lane:** A

**In short:** a file on a memory filesystem -- `/tmp`, the in-memory root,
every memfd -- keeps its bytes in memfs's own buffer, and the page cache
kept a second copy of every 16 KiB page that was read or mapped. A program
that maps a 64 MiB memfd shared, as a compositor's client does with its
buffers, used 128 MiB. Now memfs files are read past the page cache (it is
memory already), and a file that is mapped keeps its data in physical frames
of its own, which the page cache takes a reference on instead of copying:
one copy, which a write, a read and every mapping all see.

### The design

1. **memfs reads skip the page cache.** `FileSystem::caches_contents` is
   false for memfs, so the VFS reads and writes it directly
   (`read_at_routed`, `read_file_routed`, `object_read`): caching memory in
   memory only doubled it.
2. **A memfs file's data is bytes until it is mapped, frames after.**
   `MemData::Bytes(Vec<u8>)`, exactly sized, is what most files are --
   the in-memory root's many small files would cost a 16 KiB frame each
   otherwise. A mapping's first fault asks the filesystem for the page's
   frame (`FileSystem::page_frame`); memfs then turns the file into
   `MemData::Frames`, its pages in frames of its own (holes allocated as
   they are asked for), and answers the frame with a reference taken.
3. **The page cache adopts the frame** (`page_cache::get_or_fill_from`): its
   entry for the page is memfs's own frame, so the mapping, the cache and
   memfs share one copy. A `write(2)` lands in it, and the mapping sees it
   at once; a store through the mapping is in the file at once. Dropping the
   cache's entry (memory pressure, a write to a file with no shared mapping)
   only gives back a reference: memfs keeps its own.
4. **Nothing else changes in the cache's rules.** A writable shared mapping
   still marks a page dirty on its first store and saves it (§1572): for an
   adopted page that save writes a frame's bytes onto themselves, which is
   redundant and correct. A cut zeroes and drops the pages as for any file;
   memfs frees its frames past the end, the cache its references.

### The alternatives not taken

| Alternative | For | Against |
|---|---|---|
| The page cache is memfs's store, as Linux's tmpfs (`shmem`) | One mechanism for every file | memfs does not know its filesystem's id, the cache's key, until it is mounted, and works unmounted (self-tests, the conformance harness); its pages could never be evicted, a rule the cache does not have; every memfs read would go through a cache lookup |
| Every memfs file in frames | One representation | A 1-byte file costs a 16 KiB frame: the in-memory root's and `/tmp`'s small files would cost many times their size |
| Only skip the cache for reads | A one-line change, half the waste gone | A mapped file still costs twice its size, which is the case the fix is for |

### As built

- `kernel/src/fs/memfs.rs`: `MemData`, the conversion at the first
  `page_frame`, the frames freed with the node or past a cut.
- `kernel/src/fs/vfs.rs`: `caches_contents`, `page_frame` on the trait;
  the read paths skip the cache for a filesystem that does not cache.
- `kernel/src/mm/page_cache.rs`: `get_or_fill_from`, which takes the
  filesystem's frame when it offers one and fills a frame of its own
  otherwise.
- `kernel/src/fs/handle.rs`: `cached_page`, the page a mapping's fault
  gets; `kernel/src/proc/pcb.rs`'s two fault paths call it.
