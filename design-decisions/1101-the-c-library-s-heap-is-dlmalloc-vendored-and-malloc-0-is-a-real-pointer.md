## 1101. The C library's heap is dlmalloc, vendored — and `malloc(0)` is a real pointer

**Date:** 2026-09-25
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** until today every `malloc` in a SlateOS program asked the kernel
for a fresh 16 KiB page and gave it back on `free` — and so did every `Box`,
`Vec` and `String` in the Rust programs, which go through the same function. A
10-byte string cost 16 KiB and two system calls, and the kernel printed a
console line for each. The C library now uses Doug Lea's allocator (the design
glibc's descends from), which carves small blocks out of 64 KiB regions with no
system call at all. The same change makes `malloc(0)` return a usable pointer
instead of NULL, which is what programs written for Linux expect.

**What was there.** `posix/src/malloc.rs` gave each block its own `mmap`, with
a 16-byte header naming it; `free` was `munmap`. Its own module doc called that
"correct but not efficient … programs needing a real allocator can link one in
later". None did, and the cost fell everywhere, because Rust's
`std::alloc::System` on `x86_64-slateos` (`os = "linux"`, `env = "musl"`) calls
these very functions.

**What it is now.** The core of dlmalloc-rs 0.2.14 (Alex Crichton's port, which
Rust's standard library uses on wasm), vendored as
`posix/src/malloc/dlmalloc.rs` with `VENDORED.md` beside it: upstream version,
checksums, licence (MIT/Apache-2.0) and every local change. One heap, one lock.
Its source of memory, `SlateSystem`, maps and unmaps whole regions, and three of
the changes come from this kernel:

- the core never merges two neighbouring regions into one segment, because
  native `munmap` drops a mapping's record only when given that mapping's own
  base address, so a merged segment freed in one call would leave the second
  record behind;
- it never gives back part of a region, for the same reason;
- a request of 256 KiB or more gets a mapping of its own (C dlmalloc's
  `mmap_alloc`, which the Rust port lacks), so a large block goes back to the
  kernel as soon as it is freed rather than when its whole segment empties.

`fork` takes the heap lock after the `pthread_atfork` *prepare* handlers and
releases it before the *child*/*parent* ones — glibc's order — so a child never
inherits a heap frozen mid-update by a thread it does not have.

**Size zero.** `malloc(0)`, `calloc(0, n)`, `posix_memalign(&p, a, 0)`,
`aligned_alloc(a, 0)`, `memalign`, `valloc(0)` and `pvalloc(0)` now return a
unique pointer that must be freed, as both glibc and musl do. POSIX allows
either answer; ported code assumes Linux's (`if (!(p = malloc(n)))
die("out of memory")` with a legitimate `n == 0`). `realloc(p, 0)` still frees
and returns NULL (glibc's answer; musl returns a new block; C23 makes it
undefined, so no portable program depends on either).

**Alternatives.**

| Option | For | Against |
|---|---|---|
| Keep one mapping per block | simplest; a use-after-free faults at once | 16 KiB per block minimum; two syscalls per block; a console line per call |
| Write our own size-class allocator | fits the kernel exactly | a new heap is a new source of heap bugs; dlmalloc has decades of use |
| `dlmalloc` as a Cargo dependency | no vendored code | the no-merge change is inside `sys_alloc` and cannot be made through the crate's `Allocator` trait; `posix`'s first registry dependency, in every program's link |
| musl's mallocng, ported from C | metadata kept away from user data, so a heap overflow is far harder to exploit | ~1,500 lines of subtle C to port and keep in step |
| jemalloc / mimalloc | fastest under many threads; `memory management.txt` leans this way | large C code bases; need per-thread caches and cheap TLS the libc does not yet have |

**The cost of this choice** is hardening. dlmalloc keeps its bookkeeping inline,
beside user data, so a C program that writes past the end of a block can corrupt
the heap in exploitable ways; mallocng exists largely to prevent that. The
long-run choice is `deferred-questions.md` → "[D] Which allocator should the C
library's heap be in the long run?", with its triggers. Any successor replaces
only the core behind `SlateSystem` and `HeapGuard`; the C API and the system
interface stay. `known-issues.md` → `TD-D-MALLOC-HAS-ONE-LOCK-AND-INLINE-METADATA`.

**How it was tested.** Upstream's regression tests run against `SlateSystem`,
with upstream's own assertions switched on in test builds. In test builds every
new block is filled with 0xA5 and every freed one with 0x5A (glibc's
`MALLOC_PERTURB_`), because the old allocator returned zeroed memory and faulted
on use-after-free, so code relying on either could never have failed a test.
The whole `posix` suite — 20,770 tests — passes under that, in the default
order and in three shuffled ones. New tests cover size zero at every alignment,
contents across the 256 KiB threshold, a 6,000-operation mixed workload checking
every byte, four threads sharing the heap, and the `fork` lock.

**Revisit when** a program shows the heap lock in a profile (per-thread caches);
when hardening is scheduled (mallocng); or when the kernel's `munmap` can split
a mapping or `mremap` exists (then `free_part`/`remap` can say yes).
