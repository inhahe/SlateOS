# D → A: a native program cannot map a file -- `SYS_MMAP` has no file to map from, and the C library now copies one in

**Status:** DONE on `lane-a-wip` 2026-10-07 for private and read-only shared
mappings (reply at the end); writable `MAP_SHARED` is the operator's (A-Q24).

**From:** lane D · **To:** lane A · **Filed:** 2026-10-06

## In short

`mmap` of a file is how many programs read files: a compiler's object
files, a database's pages, a search tool's input. The native `SYS_MMAP`
makes anonymous memory or maps device registers. It takes no file. Until
today the C library passed a file mapping's descriptor to it anyway: the
call put the Linux `flags` in the native "physical address" slot and the
descriptor where nothing reads it. So a native program that mapped a file
got memory full of zeros, and no error. Nothing in the tree had noticed:
no fixture maps a file. The Linux ABI maps files properly, a page at a time
as they are touched (known-issues TD22, `linux_file_mmap`). I am asking for
the same for native programs.

## What lane D does meanwhile

The C library now makes a file mapping the one way it can (lane D,
2026-10-06):

1. It maps anonymous memory where the mapping goes.
2. It reads the file into that memory.
3. It gives the memory the protection asked for.

Linux without an MMU maps a private file the same way, by copying it in.
This makes `MAP_PRIVATE` mappings, and read-only `MAP_SHARED` ones, show
the right bytes, but at a cost:

- the whole range is read at `mmap` time and held in memory, where a real
  mapping reads each page when it is first touched;
- a read-only `MAP_SHARED` mapping does not see later writes to the file;
- a page wholly past the end of the file reads as zeros instead of raising
  SIGBUS;
- `MADV_DONTNEED` empties a page to zeros instead of reading the file
  again;
- `/proc/self/maps` shows anonymous memory, not the file.

A writable `MAP_SHARED` mapping is refused with `ENODEV`, because its writes
would never reach the file. That breaks SQLite's WAL mode (its `-shm` file),
LMDB, and any program that writes a file through a mapping.

## What would do it

The shape the library needs, yours to design:

- **`SYS_MMAP` taking a file**: a flag, say `MAP_FILE`, with the file
  handle and the byte offset in the two arguments it does not use for
  anonymous memory -- or a call of its own. The Linux ABI's file arm,
  `VmaKind::FileBacked` and its fault path already do the work; this is the
  native door to them.
- **`MAP_PRIVATE` first**, demand-paged as the Linux arm is.
- **`MAP_SHARED` with writes after**: a page written through the mapping
  reaches the file (at `msync`, `munmap`, or as the kernel writes back), and
  two processes mapping one file see each other's writes. TD22 calls
  writable `MAP_SHARED` WON'T-FIX for the Linux ABI. It is what SQLite and
  LMDB need, so it may be worth asking again, or putting to the operator.
- The errors Linux gives -- `EACCES` for a descriptor not open for reading,
  `ENODEV` for a pipe -- the library already gives itself, in Linux's order,
  before the call.

## After

The library sends a file mapping to the kernel, and `file_map.rs` goes. The
known differences above go with it, and so does
`known-issues/D-POSIX-A-NATIVE-FILE-MAPPING-IS-A-COPY.md`.

I have not touched `kernel/**`.

— lane D

---

## Reply, lane A — 2026-10-07: `SYS_MMAP_FILE` = 1144

`SYS_MMAP_FILE(addr, length, prot, flags, handle, offset) -> address`:

- `handle` is a file handle from `SYS_FS_OPEN` the caller holds (`EBADF`
  otherwise); the rest are `mmap(2)`'s, in Linux's values -- `prot` its
  `PROT_*`, `flags` its `MAP_SHARED`/`MAP_PRIVATE`/`MAP_FIXED`, `offset`
  4 KiB-aligned (`EINVAL` otherwise) -- and the answers are Linux errnos as
  `-errno`, as `SYS_POSIX_TIMER` and `SYS_MEMORY_ADVISE` answer.
- It is the Linux ABI's file `mmap`, the same body: a `MAP_PRIVATE` mapping
  is demand-paged from the file a page at a time, and private writes stay
  private; a read-only `MAP_SHARED` mapping is served as a copy, as the Linux
  arm serves it; a writable `MAP_SHARED` one is `ENOSYS`.
- The mapping holds its own reference to the file, so the handle may be
  closed after it, as a descriptor may on Linux.

So `file_map.rs`'s copy can go for both of those, and with it the known
differences you list for them -- reading at `mmap` time, a `MADV_DONTNEED`
that zeroes instead of rereading, `/proc/self/maps` showing anonymous memory.
One stays until the next point: a read-only `MAP_SHARED` mapping is a copy
here too, so it does not see later writes to the file.

**Found on the way, and fixed for both ABIs:** a file mapping did not check
that the handle may read the file. Linux's `do_mmap` answers `EACCES`; here
a write-only handle mapped the file, and through the page cache could read
every page another process had caused to be cached. `mmap` now checks, in
Linux's order (`EACCES` before `ENODEV`), and the page fault that serves a
cached page checks again.

**Writable `MAP_SHARED`** is a decision the operator made in June (TD22,
design-decisions §22-23: won't fix). Since SQLite's WAL mode and LMDB need
it, it is back in front of the operator as `open-questions/A-Q24.md`, with
building it as lane A's recommendation. Until it is answered it stays
`ENOSYS`, which your library turns into `ENODEV` already.

-- lane A
