## 1181. The C library's memory and string functions are SSE2 only, with no run-time choice of AVX2

**Date:** 2026-10-06
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** `memcpy`, `strlen`, `strcmp` and the rest are the functions
every program spends the most time in. Until 2026-10-06 they handled one
byte at a time. They now use SSE2, which handles sixteen bytes at a time and
which every 64-bit x86 processor has. Newer processors also have AVX2, which
handles thirty-two, and glibc picks the best one when a program starts. Ours
does not: it uses SSE2 everywhere. On long strings that leaves it at about
half glibc's speed for the searches (`strlen` 26-36 GB/s against 49 at
1 MiB); copies and fills are as fast as glibc's or faster, because they use
the processor's own copy instruction (`rep movsb`).

**The choice:**

| Option | *What changes* | For | Against |
|---|---|---|---|
| **A. SSE2 only** (chosen) | one version of each function | every x86-64 has it, and the sysroot's target spec says so (`posix/x86_64-slateos-libc.json`); one code path, which the host tests and the guard-page tests cover completely; no indirect call | long-string searches about half glibc's speed |
| B. AVX2 chosen at start-up (glibc's IFUNC shape) | two or three versions, picked by `cpuid` | about glibc's speed on long strings | an indirect call, or a relocation step the static linker and the kernel's loader do not have; every version needs its own tests, and a host without AVX2 cannot run them; `vzeroupper` discipline, and the kernel saving the wider registers (`XSAVE`) for every thread that touches them |
| C. AVX2 always | one version, wider | the speed | programs fail with an invalid-opcode fault on processors without AVX2 (before 2013 on Intel, 2015 on AMD, and many low-power parts since) |

**Why A, for now.** Most calls are short: a string of tens of bytes, where
the cost is the call and the first block, not the width of the loop. (How
ours compares with glibc there is not measured well: glibc's small calls
under WSL include the virtual machine's costs.) The gap is on long strings,
and it is within `performance-targets.md`'s "2x of glibc". B buys that
factor at the price of machinery the static toolchain lacks (`IFUNC`), of
untestable paths on hosts without AVX2, and of the kernel saving 256-bit
registers for every thread.

**When to revisit:** a SlateOS program whose profile shows `strlen`,
`memchr` or a copy as a hot spot on long buffers; or the kernel saving the
`XSAVE` state anyway (for some other reason); or dynamic linking arriving,
which brings IFUNC-like resolution with it.

**Where:** `posix/src/string.rs` ("The engines", "The scanners"),
`posix/src/wchar.rs` ("The wide scanners"); measured by
`posix/benches/mem.rs` against `posix/benches/glibc-reference.c`.
