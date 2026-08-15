# pkgconf cross-compile spike

**Result: upstream pkgconf 2.3.0 cross-compiles and links against SlateOS's
own `libc.a` with zero source changes, zero shims, and zero missing symbols —
on the first attempt.**

This is the porting-policy check from `roadmap-detailed.md` ("Porting vs.
Reimplementing" → "Try cross-compiling before you write a line") applied to
pkgconf, and it is the test that `design-decisions.md` §307 says to run before
reimplementing anything that already exists in C.

Reproduce with `./run.sh` from WSL.

## What was measured

| Step | Result |
|---|---|
| `./configure --host=x86_64-linux-musl --disable-shared --enable-static` | exit 0, clean |
| `make -j8` (against zig's musl headers) | exit 0, **no errors, no implicit-declaration warnings** |
| Relink against `toolchain/sysroot/lib/libc.a` with `-nostdlib` | exit 0 |
| Undefined symbols in the SlateOS link | **0** |
| Distinct libc symbols pkgconf actually needs | 53 (of 132 external references; the other 79 are pkgconf's own) |
| Output | 2.9 MB static `ET_EXEC`, no `PT_INTERP`, no `PT_TLS`, `_start` from our `posix` crate |

The 53 libc symbols, in full — every one already provided:

```
abort atoi bsearch calloc closedir fclose ferror fopen fprintf fputc fputs
free fwrite getc getenv isalnum lstat malloc memchr memcpy memmove memset
opendir printf putchar puts qsort readdir realloc reallocarray realpath
snprintf stat stderr stdout strcasecmp strchr strcmp strcspn strdup strlcat
strlcpy strlen strncasecmp strncmp strndup strrchr strstr strtok toupper
ungetc vfprintf vsnprintf
```

Worth noting that three of those — `strlcpy`, `strlcat`, `reallocarray` — are
BSD/glibc extensions rather than base POSIX. pkgconf ships its own fallbacks in
`libpkgconf/bsdstubs.c` and `configure` disabled them, because the musl headers
it probed declare all three; our `libc.a` then had to satisfy them for real at
link time, and did. So this is genuine coverage, not a case of upstream routing
around a gap.

## Why the result matters beyond pkgconf

`design-decisions.md` §307 argues that a real port is the only honest coverage
test a libc has, because you cannot guess which of several thousand symbols
matter. This run is the cheap, positive-outcome version of that: it took a
handful of minutes and it certified 53 symbols against real software rather
than against our own expectations.

It also settles a live question. `userspace/pkgconf/` had an in-progress Rust
reimplementation (5 modules, 3,143 lines, 112 passing tests) which prompted the
operator's question — *"if upstream pkgconf doesn't build against our libc,
could that be taken as a suggestion to improve our libc for it and for future
apps instead?"* — that became §307. The premise turned out not to apply: nothing
needed improving. Per the policy the port wins, and the rewrite is preserved but
not merged, on the branch `wip/pkgconf-rust-parked`.

That rewrite is **unfinished**, and is now labelled as such in two places so
nobody picks it up expecting a nearly-done job: `userspace/pkgconf/STATUS.md`
and `known-issues.md` →
`TD-PKGCONF-THE-RUST-REWRITE-IS-UNFINISHED-AND-SUPERSEDED-BY-THE-UPSTREAM-PORT`.
Measured state: 112/112 tests pass and it *does* build for `x86_64-slateos`
(an earlier note of mine claiming otherwise was wrong), but it implements 34 of
upstream's 62 long options, clippy is red with 9 errors, and it has never run
under the kernel. Note the shape of that comparison — the port reached a
working binary in minutes; the rewrite is 3,143 lines in and still roughly half
an implementation.

## What this spike does *not* establish

Linking is not running. The binary has never been executed under the SlateOS
kernel, so the remaining unknowns are:

- **`realpath`, `lstat`, `stat`, `opendir`/`readdir` behaviour on our VFS.**
  pkgconf leans on these to canonicalise and walk `PKG_CONFIG_PATH`. They
  resolve at link time; whether they return what pkgconf expects is a runtime
  question.
- **Segment alignment.** The `PT_LOAD` headers carry `p_align` of 0x1000 and
  0x2000, while SlateOS uses 16 KiB pages. The loader's handling of
  sub-page-size alignment is worth confirming on the first on-target run.
- **`getenv` and the environment block**, which pkgconf uses heavily
  (`PKG_CONFIG_PATH`, `PKG_CONFIG_SYSROOT_DIR`, `PKG_CONFIG_LIBDIR`).

The natural next step is an on-target ring-3 self-test in the same shape as the
`services/fastpy-*` binaries: run `pkgconf --version` and `pkgconf --cflags` on
a fixture `.pc` file and assert the output, false-pass-proof.

## Method notes

- **zig cc**, matching the bash spike and fastpy's `compiler/toolchain.py`.
  SlateOS's `libc.a` is built to the musl ABI, so compiling against zig's musl
  headers and linking against our archive is legitimate rather than a fudge.
- **`$CC` must not contain spaces.** autotools word-splits it, and this repo
  lives under `D:\visual studio projects\`. The bash spike lost a run to this,
  dying with a misleading "C compiler cannot create executables"
  (`CROSS_CONFIGURE_EXIT=77`). The `/tmp/zigcc` wrapper exists to keep the
  space out of `$CC`.
- **`libstubs.a` is deliberately not linked.** It and `libc.a` are both
  Rust-built and each carries a panic handler, so together they collide on
  `__rustc::rust_begin_unwind`. `libc.a` alone covers pkgconf.
- **`libc.a` is listed twice** rather than wrapped in `--start-group`; its
  intra-archive references are not topologically ordered and a second pass is
  cheaper.

## Comparison with the bash spike

`scripts/bash-spike/` is the same experiment on much harder input, and the two
together are the argument for §307:

| | bash 5.2 | pkgconf 2.3.0 |
|---|---|---|
| Symbols referenced | 2,030 | 132 (53 from libc) |
| Missing on first SlateOS link | 3 (`killpg`, `eaccess`/`euidaccess`, `__fpurge`) | **0** |
| Source changes needed | 0 (one `configure` cache var) | 0 |
| Blocked by | a kernel gap (no suspend → no SIGTSTP/SIGCONT) | nothing found yet |

Two ports, one of which found three real libc gaps that were then implemented
for good in `posix/src`, and neither of which needed the application rewritten.
