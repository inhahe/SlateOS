## D-POSIX-SHARED-ANONYMOUS-MEMORY-IS-NOT-SHARED-AFTER-FORK — `mmap(MAP_SHARED | MAP_ANONYMOUS)` then `fork` gives the child a copy, not the same memory (lane D, 2026-10-06)

**Status:** OPEN -- both halves are written: lane D's (the library passes the flag, 2026-10-07) and lane A's (the kernel honours it, on `lane-a`). It closes when lane A's kernel is on `main` and `services/ctest-shared-anon` passes in a lane D boot.

**In short:** the everyday POSIX way to share memory with a child process is
to map anonymous memory with `MAP_SHARED`, then `fork`: both processes then
see each other's writes. For a native SlateOS program the child gets a
private copy instead, so writes on one side are never seen on the other,
and nothing says so. The native `SYS_MMAP` on `main` has no way to ask for
shared memory. Lane A's branch adds one (`MAP_SHARED`, bit 7: committed
frames that a `fork` maps into the child too), and asked the C library to
pass it (`requests/a-d-libc-mmap-should-say-map-shared-and-translate-prot.md`,
on `lane-a`).

**Where:** `posix/src/mman.rs` -- `mmap`'s anonymous path.

**Lane D's half, done 2026-10-07:** `native_flags` passes the native
`MAP_SHARED` for a `MAP_SHARED` anonymous mapping, and never `MAP_LAZY`,
which the kernel refuses with it. What Linux 6.6.87 refuses for anonymous
memory is refused first, measured: `MAP_SHARED_VALIDATE`, and
`MAP_SHARED | MAP_GROWSDOWN`, both `EINVAL`. The kernel on `main` ignores
flag bits it does not know, so until lane A's kernel is there the mapping
is private, as before -- nothing that worked stops working.
`services/ctest-shared-anon` checks the two halves together -- a child's
writes reach the parent on a shared mapping's first and last page and not
on a private one, a running child sees the parent's write and answers -- and
passes on Linux; it is on lane A's generic rung's list of fixtures waiting
(`requests/d-a-one-rung-for-every-c-fixture.md`). The other half of lane A's
request, `prot` translated rather than passed whole, was done on
2026-10-06: `PROT_SEM` no longer reaches the native call as `MAP_NOCACHE`.

**Until then:** a program that shares memory with a child this way works
only if the child never writes what the parent reads (or the other way
round). Threads are unaffected: they share the one address space.
