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
2. **A memfs file's data is bytes while it is small and unmapped, frames
   otherwise.** `MemData::Bytes(Vec<u8>)`, exactly sized, is what most
   files are -- the in-memory root's many small files would cost a 16 KiB
   frame each otherwise -- up to `BYTES_MAX` (64 KiB). A file is kept in
   `MemData::Frames`, its pages in frames of its own (holes allocated as
   they are asked for), from the write that takes it past that, and from a
   mapping's first fault, which asks the filesystem for the page's frame
   (`FileSystem::page_frame`); memfs answers it with a reference taken. So
   that conversion never copies more than 64 KiB, and never doubles a large
   file's memory while it runs. A file in frames stays in frames.
3. **The page cache adopts the frame** (`page_cache::get_or_fill_from`): its
   entry for the page is memfs's own frame, so the mapping, the cache and
   memfs share one copy. A `write(2)` lands in it, and the mapping sees it
   at once; a store through the mapping is in the file at once. Dropping the
   cache's entry (memory pressure, a write to a file with no shared mapping)
   only gives back a reference: memfs keeps its own.
4. **An adopted page is the file, and the cache treats it so.** Its entry
   is marked adopted. A writable shared mapping's first store to it is let
   through without marking it dirty -- the store is in the file, and there
   is nothing to save -- and a change to the file is not copied into it, as
   it is in it already. (Saving it would copy the frame out and back under
   the filesystem's lock, putting older bytes back over a store a mapping
   made between the two.) A cut drops the pages as for any file; memfs
   frees its frames past the end, the cache its references.

   What a save does besides copying is still done, at the next save: the
   store marks the file written through a mapping
   (`MarkDirty::Adopted`, `FileState::mapped_written`), and the next
   `writeback` -- `msync`, `fsync`, the last writable unmapping, the
   background pass -- sets the file's modification and change times and
   announces the change once (`Vfs::object_mapped_write`), as a save of
   copied pages does. Not at the fault: that would be once per 4 KiB part,
   thousands of announcements for one mapping's writes. (POSIX lets the
   times be marked at any point up to the next `msync`.)
5. **A file's bytes are charged as whole pages.** The size limit counts a
   file kept as bytes by the pages they reach, not by the byte -- the frames
   turning them into frames will take -- as Linux's tmpfs charges whole
   pages (16 KiB ones here, as on a 16 KiB-page Linux). So a mapping's first
   fault never needs room the file was not charged already: mapping a file
   in a full filesystem cannot fail, and mapping many small files cannot
   take one past its limit. Only a hole a fault fills counts as new, and is
   refused past the limit with `SIGBUS`, as tmpfs answers it. The cost: a
   size-limited memfs holds a quarter as many files of 4 KiB or less as a
   4 KiB-block count would let it. *Decided by:* Claude (autonomous), on a
   review's finding that charging bytes let mapped files reach about four
   times the limit.

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
