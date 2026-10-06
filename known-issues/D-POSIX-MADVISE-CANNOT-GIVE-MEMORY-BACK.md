## D-POSIX-MADVISE-CANNOT-GIVE-MEMORY-BACK — a native program's `madvise(MADV_DONTNEED)` and `MADV_WIPEONFORK` are refused, so memory is never given back (lane D, 2026-10-06)

**Status:** OPEN -- waiting on lane A (`requests/d-a-a-native-program-has-no-madvise.md`).

**In short:** programs that free a lot of memory tell the kernel so with
`madvise(MADV_DONTNEED)`: the memory goes back to the system, and reads as
zeros if used again. Security libraries use `MADV_WIPEONFORK` to have a
child process see a range as zeros. A native SlateOS program has no
`madvise` to reach. Until 2026-10-06 the C library answered success and did
nothing, and both promises were silently broken: allocators could hand out
old bytes as zeros, and a forked child could reuse its parent's random
numbers. Both are now refused with `EINVAL`, so programs take their safe
paths, which work. What still costs: memory a program frees through
`madvise` is never returned, so a long-running program's footprint only
grows.

**Where:** `posix/src/mman.rs` -- `madvise`, `MADV_NOT_YET` (the five
refused: `MADV_DONTNEED`, `MADV_DONTNEED_LOCKED`, `MADV_REMOVE`,
`MADV_WIPEONFORK`, `MADV_KEEPONFORK`). `posix_madvise`'s
`POSIX_MADV_DONTNEED` is a hint in POSIX, and stays an accepted one, as
glibc's is.

**The proper fix:** a native `madvise` -- or native calls for
reclaim-and-zero, which the Linux ABI's `madvise_reclaim` already does, and
for the wipe-on-fork flag. Then the library passes the five through.

**The same request asks lane A** to stop the Linux ABI accepting
`MADV_WIPEONFORK` without wiping. That one is a live security bug for
Linux-ABI programs, not this library's.
