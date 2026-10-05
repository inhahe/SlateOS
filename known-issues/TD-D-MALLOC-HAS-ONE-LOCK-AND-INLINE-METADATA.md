### [D] TD-D-MALLOC-HAS-ONE-LOCK-AND-INLINE-METADATA — 2026-09-25 — OPEN

**Status:** OPEN — accepted costs of the allocator adopted 2026-09-25
(design-decisions §1101), written down so they are found rather than
rediscovered. Nothing is broken.

**In short:** the C library's heap — every `malloc`, and every `Box`, `Vec` and
`String` in the Rust userland — is now Doug Lea's allocator behind a single
lock. Two things about it are weaker than they could be: every thread in a
process takes that one lock to allocate, so a program with many busy threads
queues on it; and the allocator keeps its bookkeeping beside the program's data,
so a C program that writes past the end of a block can corrupt the heap in ways
an attacker can use.

**Where:** `posix/src/malloc.rs` (`HEAP`, `HeapGuard`); the vendored core,
`posix/src/malloc/dlmalloc.rs` (inline chunk headers, as in glibc).

**Also missing, same place:** ~~`malloc_trim`, `mallinfo`/`mallinfo2` and
`malloc_stats`~~ — **added 2026-09-25**, on upstream's `trim` and a port of C
dlmalloc's `internal_mallinfo` walk (`Dlmalloc::stats`, VENDORED.md local change
7). `malloc_trim` can release only whole free segments, because `free_part`
cannot unmap part of a mapping. `mallinfo`'s `int`s saturate rather than wrap.
`malloc_stats` omits glibc's two "max mmap" lines, which dlmalloc does not
track. Still missing: `mallopt` and `malloc_info`. No port has linked against
either (bash, make, pkgconf, CMake and CPython all link with nothing missing),
and `mallopt` would need the mapping threshold, now a constant, to become a
setting.

**Proper fix:** measure first. Contention → per-thread caches in front of the
shared heap. Hardening → an allocator with out-of-band metadata (musl's
mallocng) or a size-class design. The choice between them is
`deferred-questions.md` → "[D] Which allocator should the C library's heap be
in the long run?", with its triggers. Any of them replaces only the core behind
`SlateSystem` and `HeapGuard`.
