## 970. "This page is shared" is a bit in the page-table entry, set where shared memory is mapped, and fork, compaction and futex keys all read it

**Date:** 2026-09-27 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A

**In short:** some memory is meant to be seen by more than one party at once:
a shared-memory region two programs use, the ring a program shares with the
kernel, a buffer a device reads. Three parts of the kernel have to treat such
memory differently from a program's private memory:
- `fork` must not give the child a private copy of it;
- compaction must not move it;
- a futex on it must be one futex in every process that maps it.

Until now nothing recorded the fact, and all three got it wrong. This
decision records it in one place, a spare bit in the page-table entry, set
at the five places such memory is mapped, and has all three read it there.

**The question.** Where does "shared by design" live?

**Options.**

| option | pro | con |
|---|---|---|
| **A spare PTE bit, `PageFlags::SHARED` (bit 10)** -- chosen | Read where the three consumers already are: fork walks page tables, not VMAs; a futex key is one page walk, no lock. Covers mappings that have no VMA at all (device memory, DRM buffers). Set once, at map time, by the code that knows | Invisible to `/proc/<pid>/maps` (which reads VMAs). Every code path that rewrites a user leaf must keep the bit, so a new rewriter can drop it by accident; `mprotect`'s rule (`user_protect_flags`) keeps all non-permission bits for that reason |
| A `VmaKind::Shared` variant | Visible in `/proc/<pid>/maps`; one record per mapping, not per page | `VmaKind::Fixed` would have to be split and every `match` on it revisited; device and DRM mappings have no VMA, so they would stay unmarked; fork would need the VMA list in a page-table walk that does not have it; a futex key would take the process table lock on every wait and wake |
| Linux's `FUTEX_PRIVATE_FLAG` as the switch (futexes only) | Matches Linux exactly: a private-flagged futex is (mm, address) even on a shared page | Only answers the futex third of the question; the native futex calls take no flags, so the ABI would have to grow one; a program that forgot the flag would get physical keys on private pages, where copy-on-write frames shared after a fork would cross-wake parent and child |

**Why the bit.** Fork and the compactor work in page tables and cannot
cheaply ask the VMA list. The futex fast path must not take the process
table lock. And the fact is known exactly once, where the shared frame is
mapped. The cost -- rewriters must preserve the bit -- is paid once, in the
one rule `mprotect` now uses for a user leaf, and is pinned by its
self-test.

**Revisit if** `/proc/<pid>/maps` needs to show shared mappings as `s`: add
the VMA-side record then, *in addition*, set at the same five sites.
