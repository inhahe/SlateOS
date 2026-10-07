## D-POSIX-SHARED-ANONYMOUS-MEMORY-IS-NOT-SHARED-AFTER-FORK — `mmap(MAP_SHARED | MAP_ANONYMOUS)` then `fork` gives the child a copy, not the same memory (lane D, 2026-10-06)

**Status:** OPEN -- the kernel half is written, on `lane-a`, and reaches `main` with lane A's next publish; then lane D passes the flag.

**In short:** the everyday POSIX way to share memory with a child process is
to map anonymous memory with `MAP_SHARED`, then `fork`: both processes then
see each other's writes. For a native SlateOS program the child gets a
private copy instead, so writes on one side are never seen on the other,
and nothing says so. The native `SYS_MMAP` on `main` has no way to ask for
shared memory. Lane A's branch adds one (`MAP_SHARED`, bit 7: committed
frames that a `fork` maps into the child too), and asks the C library to
pass it (`requests/a-d-libc-mmap-should-say-map-shared-and-translate-prot.md`,
on `lane-a`).

**Where:** `posix/src/mman.rs` -- `mmap`'s anonymous path passes only
`native_access(prot)`.

**What lane D does when lane A's kernel is on `main`:** pass the native
`MAP_SHARED` for `MAP_SHARED` and `MAP_SHARED_VALIDATE` anonymous mappings
(never with `MAP_LAZY`, which the kernel refuses with it), and add a fork
check to a ring-3 fixture -- a write in the child seen by the parent. The
other half of lane A's request, `prot` translated rather than passed whole,
was done on 2026-10-06: `PROT_SEM` no longer reaches the native call as
`MAP_NOCACHE`.

**Until then:** a program that shares memory with a child this way works
only if the child never writes what the parent reads (or the other way
round). Threads are unaffected: they share the one address space.
