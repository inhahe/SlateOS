## DQ4 — Which allocator should the C library's heap be in the long run? — deferred 2026-09-25 (lane D)

**In short:** every SlateOS program's memory allocation now goes through Doug
Lea's allocator (design-decisions §1101) — mature, fast for ordinary programs,
and a large improvement on what it replaced. It has two known weaknesses: every
thread in a program shares one lock to allocate, and its bookkeeping sits right
next to program data, so a buggy or attacked C program can corrupt it in
exploitable ways. Whether either matters enough to replace it — and with what —
needs measurements this project cannot take yet. Nothing is wrong today and
nothing is waiting on an answer.

**Why this is not in `open-questions.md`.** Both weaknesses are hypothetical
until a real workload shows them, and the options differ mainly in performance
and hardening that can only be compared on SlateOS itself. Asking now would be
asking for a guess.

**The choice, when it arrives:**

| Option | *What changes* |
|---|---|
| Keep dlmalloc, add per-thread caches in front (glibc's tcache shape) | *Busy multi-threaded programs stop queueing on the heap lock; heap-overflow exploits stay as easy as on glibc.* |
| Port musl's mallocng | *A heap overflow in a C program becomes far harder to turn into an exploit; allocation gets somewhat slower.* |
| A mimalloc/jemalloc-style size-class allocator | *Fastest under many threads and closest to where `memory management.txt` leaned; the largest port, and hardening depends on which.* |

Any of them replaces only the core behind `posix/src/malloc.rs`'s `SlateSystem`
and `HeapGuard`; programs see no difference in interface.

**Trigger to promote this into `open-questions.md`:** whichever comes first —
(a) a program on SlateOS shows the heap lock in a profile, or allocation in
general as a hot spot (the heap benchmark belongs in the same commit that
measures it); (b) hardening against memory-corruption exploits is scheduled for
userspace; (c) a heap-corruption bug in a ported C program is traced to inline
metadata being overwritten.

**Related:** `known-issues.md` → `TD-D-MALLOC-HAS-ONE-LOCK-AND-INLINE-METADATA`;
`memory management.txt` (the operator's own allocator discussion, which ends
recommending geometric size classes).
