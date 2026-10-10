## 1538. `madvise` keeps its promises at 4 KiB, and native programs reach it through one door

**Date:** 2026-10-07 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** some `madvise` advice is a promise that programs build on, not
a hint. Memory allocators give pages back with `MADV_DONTNEED` and then skip
clearing what `calloc` hands out, because Linux guarantees those pages read
as zeros afterwards. This kernel dropped only whole 16 KiB frames inside the
range, so the 4 KiB requests allocators actually make usually changed
nothing, and the "zeroed" memory still held old data. Now exactly the 4 KiB
pages asked for are dropped. `MADV_REMOVE` now zeroes shared memory, and
native programs, which had no `madvise` at all, get the same call as
`SYS_MEMORY_ADVISE` (1140). Lane D's
`requests/d-a-a-native-program-has-no-madvise.md`; the fork-time half is
§1537.

**What the next touch sees after `MADV_DONTNEED`** (also `_LOCKED`, and
`MADV_FREE`, done eagerly):

| memory | Linux | here, before | here, now |
|---|---|---|---|
| private anonymous | zeros | zeros for whole frames only, old bytes at 4 KiB | zeros, 4 KiB exact |
| private file mapping | the file's bytes (private copy discarded) | the private copy, unchanged | the file's bytes |
| shared memory | the same shared page | a *private* zero page -- sharing silently lost | the same shared page (kept) |

The 4 KiB case needed one fault-path change: a 16 KiB frame one region covers
but only partly present must be filled sub-page by sub-page. The whole-frame
path would have answered "resolved" without mapping the page, and the access
would have faulted forever (§1537's sub-page resolver does the filling, and
reuses the frame only when no other address space holds it).

**Choice 1 -- shared memory is kept, not dropped.** Linux drops this
process's view and finds the same shared page on the next touch. A region
here does not record which frame it shares, so a dropped page could only
come back private and empty. Keeping it gives the same observable answer.
*What changes:* `MADV_DONTNEED` on shared memory frees nothing; nothing
reads differently from Linux.

**Choice 2 -- a range with unmapped parts still answers 0 for `DONTNEED`.**
Linux answers `ENOMEM`. But the kernel's loader maps a program's own segments
and its initial stack without region records, where Linux has regions, so
`ENOMEM` would refuse requests Linux allows. *What changes:* a `DONTNEED`
partly over genuinely unmapped memory succeeds here and fails on Linux. No
known program depends on that failure. (`MADV_REMOVE` and the fork advice,
whose effects need the regions, do answer `ENOMEM`.)

**Choice 3 -- `MADV_REMOVE` zeroes in place.** Linux punches a hole in the
memory behind a shared mapping: every sharer reads zeros, and the pages are
freed. Shared memory here is committed for its lifetime, so the bytes are
zeroed in place: every sharer reads zeros, but no memory is freed. On private
memory it is `EINVAL` (anonymous) or `EACCES` (file), as on Linux.

**Choice 4 -- the native door speaks Linux.** `SYS_MEMORY_ADVISE` is the Linux
ABI's `madvise` itself -- Linux's `MADV_*` values, `-errno` answers -- as the
device door (`SYS_DEVICE_*`, §1520) is, so the C library passes it through
with no table of its own. A native vocabulary would be a second copy of the
same eleven meanings to keep in step.
