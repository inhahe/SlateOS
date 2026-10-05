## B-SIGACTION-USED-THE-KERNEL-STRUCT-LAYOUT-NOT-THE-C-LIBRARYS (lane B, 2026-09-09) -- FIXED the same day

**In short:** the structure a C program fills in to install a signal handler has
two different field orders on x86_64 -- one the kernel uses, one the C library
uses -- at the same total size. Our libc used the kernel's. Handlers still ran,
because the handler is the one field both put first; everything else the
program asked for was read out of the wrong bytes.

**Where.** `posix/src/signal.rs`, `struct Sigaction` and `sigaction()`.

**Measured**, on Linux 6.6 with `offsetof`, and against musl at compile time
with `zig cc --target=x86_64-linux-musl`:

| | offsets |
|---|---|
| kernel (`rt_sigaction`) -- what we had | handler 0, flags 8, restorer 16, mask 24 |
| glibc and musl -- what a C caller passes | handler 0, mask 8, flags 136, restorer 144 |

**Effect.** `sa_flags` was read from the first word of the caller's `sa_mask`,
zeroed by `sigemptyset`, so every flag was silently dropped and read back as
zero. `sa_mask` was read sixteen bytes into itself -- harmless while empty,
wrong otherwise. `oldact` was written in the wrong order, so a caller reading
its own flags back got bytes from the middle of its own mask.

**Why it went unnoticed.** No `sa_flag` was implemented, so a dropped flag and
a stored one behaved identically. `SA_ONSTACK` (design-decisions 1009), added
hours earlier, is the first flag a C program can set and observe. Also: no Rust
test could see it, because Rust builds the struct by field name and agrees with
itself whichever order it picks -- the same reason the `long double` fixtures
are written in C.

**Worse than unnoticed: certified.** A test pinned
`offset_of!(Sigaction, sa_flags) == 8` under a comment reading "glibc x86_64".

**Fixed** by reordering the struct and narrowing `sa_flags` to `u32` (the C
declaration is `int`; a `u64` would land on the same offsets by absorbing the
padding and would read four bytes belonging to the caller).
`services/ctest-altstack/` gained a C-side round-trip check that would have
caught it: install a flag and a mask, read them back, assert both survive.

**Adjacent, checked at the same time and clean:** `stack_t` matches musl at
`ss_sp` 0, `ss_flags` 8, `ss_size` 16, size 24.
