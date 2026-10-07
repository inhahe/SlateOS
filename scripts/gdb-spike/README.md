# gdb-spike — does upstream GDB link against SlateOS's libc?

**Yes, as of 2026-10-07: GDB 18.1 and gdbserver both link with nothing
missing.** The first run stopped earlier, in the headers rather than the
library: GNU's bfd asks for `_GNU_SOURCE`, and under it glibc declares the
large-file names (`off64_t`, `fopen64` ...) where musl's headers no longer do.
`posix/include/features.h` now gives glibc's rule
(known-issues-resolved/D-POSIX-GNU-SOURCE-DID-NOT-TURN-ON-THE-LARGE-FILE-NAMES.md).

| | gdb | gdbserver |
|---|---|---|
| undefined symbols | **0** | **0** |
| duplicate symbols | **0** | **0** |
| link exit | 0 | 0 |
| binary, unstripped | 110,986,232-byte static `ET_EXEC` | 11,052,424-byte static `ET_EXEC` |
| `--strip-debug` / `--strip-all` | 14,323,448 / 11,874,664 | 3,226,448 / 2,475,936 |

GMP 6.3.0 and MPFR 4.2.2 configure and build against our libc on the first
attempt, and so does all of GDB -- bfd, opcodes, libiberty, readline, gnulib,
gdbsupport, gdbserver. The make log's sixteen `undefined symbol: main` lines
are configure probes linking a test without a `main`, which any toolchain
refuses. Both binaries carry this library's Rust machinery (2,606 Rust
symbols in `gdb`, `rust_begin_unwind` among them) and none of musl's own,
so the link is against ours.

Run `./run.sh` from WSL to reproduce. It is the "try the port before you write
a line" step from `roadmap-detailed.md`'s *Porting vs. Reimplementing* policy,
applied to the debugger the operator asked for in design-decisions.md 1050
("do we have a capable debugger, like cdb? We should port one or more of
those"), which `roadmap.md` gives to lane D. It is shaped like
`scripts/make-spike/run.sh` and `scripts/cmake-spike/run.sh`; the three answer
the same question.

## What is measured

GDB **18.1**, unmodified, with **GMP 6.3.0** and **MPFR 4.2.2** (GDB will not
configure without both), cross-configured with `--build` and `--host` so that
configure runs nothing it builds. Everything is compiled with zig's `cc` and
`c++` against musl's headers with `posix/include` in front of them, and every
link -- configure's tests included -- is zig's `ld.lld` against
`toolchain/sysroot/lib/libc.a` with zig's `libc++`, `libc++abi`, `libunwind`
and `compiler_rt` around it (`slate_make_link_wrappers` in
`scripts/lib/worktree.sh`). GMP and MPFR go through the same wrappers, so what
*they* need from the C library counts too.

Two programs are measured, each by its own link: `gdb` itself, and
`gdbserver`, the small program that runs beside the one being debugged and
which GDB drives over a pipe or a socket.

Left out, each for a stated reason (the comments in `run.sh`): the simulators
and the profiler that share GDB's tarball; the in-process agent, which is a
shared library; the Python and Guile extension languages; and the optional
libraries GDB uses when it finds them (expat, lzma, zstd, debuginfod,
babeltrace, Intel PT, xxhash, source-highlight) -- each would be a port of its
own, and configure must not find the host's copy and link a Linux library into
a SlateOS binary. There is no ncurses: GDB's configure falls back to its own
`stub-termcap.o` and turns the text UI off.

## What this does not answer

Whether GDB can debug anything. A debugger controls another process through
`ptrace(2)` and `/proc/<pid>/mem`, and SlateOS has neither for native
programs: the library's `ptrace` answers `ENOSYS` after its argument checks.
That half is the kernel's, asked of lane A in
`requests/d-a-a-debugger-needs-ptrace-for-native-programs.md`, which lists
what GDB 18.1 and LLDB 20.1.8 call, read from their source. Until it exists,
a GDB that links can examine a program without running it -- symbols, types,
disassembly -- and debug a remote target as a client, and no more.

## Sources

Pinned in `scripts/lib/worktree.sh`, each by its sha256 and each attested by
packagers independent of the download server and of each other (the comment
above each pin names them).
