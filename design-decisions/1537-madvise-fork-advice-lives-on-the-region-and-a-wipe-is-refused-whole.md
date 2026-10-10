## 1537. `madvise`'s fork advice lives on the region, and a wipe is refused whole

**Date:** 2026-10-07 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** a program can ask that, when it forks, the child get some of its
memory zeroed instead of copied (`MADV_WIPEONFORK` -- used for secrets such as
a random-number generator's state, which a child must not repeat) or not get
it at all (`MADV_DONTFORK`). The kernel used to say "yes" to both and do
neither, so a forked child of a program using BoringSSL generated its
parent's random numbers (lane D's report). Now the request is recorded on the
memory region itself and honoured by `fork`, page by 4 KiB page. Three smaller
choices came with it; each is listed with what it trades.

**The mechanism.** `Vma` gained `fork: ForkPolicy` -- two independent flags,
`wipe` and `dont_copy`, as Linux has `VM_WIPEONFORK` and `VM_DONTCOPY`
(`MADV_DOFORK` clears only `dont_copy`, so a page both wiped and not copied is
wiped again once copied again). `madvise` splits regions at the request's
4 KiB boundaries and sets the flag (`pcb::set_fork_policy`). `fork` hands the
non-copied ranges to `clone_address_space_cow`, which leaves those 4 KiB pages
out of the child (the parent's stay exactly as they were: writable, not
copy-on-write), and `fork_create` drops `dont_copy` regions from the child's
list. A wiped region stays in the child, empty, and fills with zeros on first
touch. A piece split off a region keeps its policy (`vma_subrange`), so
`mprotect` and `munmap` carry it; `exec` builds new regions and so clears it,
as Linux does.

**Choice 1 -- a wipe over any memory it does not apply to is refused whole.**
`MADV_WIPEONFORK` applies to private anonymous memory only; Linux answers
`EINVAL` for anything else, *after* changing every region before the first
unsuitable one. Here nothing changes and the answer is `EINVAL`.

| | refuse whole (chosen) | Linux's partial apply |
|---|---|---|
| caller sees | all or nothing | a prefix applied, no way to tell how much |
| Linux compatibility | differs only in a request that is already an error | exact |

*What changes:* after a failed `madvise(MADV_WIPEONFORK)` over mixed memory,
no page is marked, where Linux would have marked a prefix. No known program
depends on the prefix.

**Choice 2 -- memory the kernel's loader mapped answers `ENOMEM`.** The
program's own segments and its initial stack are mapped by the kernel's ELF
loader without region records, so there is nothing to put the flag on. A
request covering them answers `ENOMEM`, Linux's answer for unmapped memory,
after marking whatever regions it does cover -- where Linux has regions there
and would mark them (or answer `EINVAL` for a wipe of a file-backed segment).
*What changes:* `madvise(MADV_DONTFORK)` on the program's own text or initial
stack fails instead of succeeding. Nothing known does that; programs mark
memory they `mmap`, which always has a region. The alternative -- inventing
regions for the loader's mappings on demand -- would give the stack-growth
code a region it does not expect.

**Choice 3 -- `brk` grows by adding a piece, merged with the one below when
nothing tells them apart.** Growth used to replace the whole heap region with
a fresh one, which silently dropped anything recorded on the heap -- an
`mprotect`, and now a wipe, which the next fork would then have copied. It now
adds only the new span (`pcb::add_vma_merging`), merged into the heap's top
piece when that piece has the same flags and fork policy, as Linux's
`vma_merge` does. *What changes:* a heap that was partly `mprotect`ed or
marked shows as several `[heap]` lines in `/proc/<pid>/maps`, as on Linux; a
plain heap is still one.

**Found on the way, fixed with it** (each a bug before this change, made more
reachable by it): the sub-page fault resolver reused the frame of any present
sibling sub-page, so a page filled beside a frame shared with another process
-- a fork's parent, the page cache -- was carved out of that process's memory;
it now reuses only a frame this address space alone holds. `munmap` kept a
frame's reference while *any* sibling was mapped rather than any sibling *in
that frame*, leaking frames in groups whose sub-pages point into different
frames. And resident-set accounting now counts frames, not groups, wherever
such a group is made or taken apart.
