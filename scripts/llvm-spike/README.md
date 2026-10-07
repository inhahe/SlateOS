# llvm-spike — LLVM's opt, llc and ld.lld on SlateOS, linked against SlateOS's libc

**What it is for.** The fastpy compiler on SlateOS (design-decisions §1050)
writes LLVM IR and needs LLVM's optimizer, code generator and linker to make a
program of it. On Windows and WSL it reaches LLVM through llvmlite, which
loads a shared library -- and SlateOS has no dynamic linker. So on SlateOS
fastpy runs the three tools instead
(`requests/b-d-fastpy-on-slateos-needs-llvm-tools.md`):

    opt -passes='default<O2>' -inline-threshold=225 ...
    llc -O2 -filetype=obj -mtriple=x86_64-unknown-linux-musl -mcpu=x86-64 ...
    ld.lld -static --no-dynamic-linker -e _start -o OUT ... -L/usr/lib/x86_64-slateos -lc

They are also the first step of a Rust toolchain: rustc is an LLVM front end.

`./run.sh` (from WSL) builds them; `./slatelink.sh` links them again against
the current `libc.a` in a minute, which is what the rootfs recipe runs when
`libc.a` moves on. The image carries them at `/bin/opt`, `/bin/llc` and
`/bin/ld.lld`, with `libc.a` at `/usr/lib/x86_64-slateos/`.
`services/ctest-llvm-tools` runs them on SlateOS.

## What was measured

**They link: 0 undefined and 0 duplicate symbols**, with every link of the
build -- configure's checks included -- made against our `libc.a`, with zig's
C++ runtime ahead of it and zig's compiler runtime behind it, and nothing else
(2026-10-01).

| bytes | opt | llc | ld.lld |
|---|---|---|---|
| as linked | 734,704,768 | 736,448,648 | 831,974,752 |
| staged (`--strip-debug`, symbols kept) | 60,438,832 | 60,505,664 | 67,413,944 |

Each is a static `ET_EXEC` that carries the SlateOS note, with `EI_OSABI`
System V and no `PT_INTERP` or `PT_GNU_PROPERTY` -- what the kernel's loader
runs as a native program. None contains any of musl's internal functions.

That zero says more than the earlier ports' zeros. Nothing was on the line
to fill a gap: a function our libc lacked would have been reported missing,
where behind zig's cc driver musl would have supplied it.

**The image grows by 188 MB** with them: three static binaries, each with
its own copy of the LLVM libraries it uses. In LLVM 20.1.8 only lld can be
built as a multi-call driver (`GENERATE_DRIVER`), so opt and llc cannot share
a binary with it.

**What this does not answer:** whether they run. A clean link says nothing is
missing; `services/ctest-llvm-tools` asks the rest on SlateOS itself --
`--version` from each, and an IR `main` returning 42 through all three, run.

## How it is built

LLVM **20.1.8**, the last LLVM 20 release and the LLVM llvmlite 0.47 carries,
pinned by sha256 in `scripts/lib/worktree.sh`. Unmodified. X86 only, release,
static; no zlib, zstd, libxml2, libedit or threads -- opt, llc and a static
link need none of them, and the first port is better with fewer moving parts.

1. `llvm-tblgen` and `llvm-min-tblgen` for the host, since LLVM generates
   code with them as it builds.
2. The cross build, through `slate_make_link_wrappers`
   (`scripts/lib/worktree.sh`): zig's cc and c++ to compile, against musl's
   headers as every port does, and zig's `ld.lld` itself to link -- every
   link the build makes, configure's checks included -- against our
   `libc.a`, with zig's C++ and compiler runtimes around it in zig's own
   order, and nothing else.
3. `slatelink.sh`: link what is missing or older than `libc.a`, strip debug
   information but not symbols, check that each tool carries the SlateOS
   note the kernel runs native programs by, and stage it.

## Four ways the first build was wrong, all measured

1. **The library paths split.** The libraries were cmake's
   `CMAKE_<LANG>_STANDARD_LIBRARIES`, which cmake puts on the link line
   unquoted, so the worktree's path ("visual studio projects") reached the
   linker as three words, and nothing linked.
2. **Configure tested musl.** cmake does not pass that variable into its
   checks. They ran `zig cc -static -nostdlib ... -lm`, and zig answers `-lm`
   with its own musl, so every `HAVE_*` LLVM cached was musl's -- or a false
   "no" where a check named no library and so linked none.
3. **zig's driver puts musl behind every link.** Measured with `zig cc -v`
   (zig 0.13.0): `-nostdlib`, `-nodefaultlibs` and `-nostdlib++` all leave
   zig's musl `libc.a` on the line, after ours. Our `libc.a` resolves first,
   so musl only fills gaps -- but a function our libc lacked would have been
   supplied by musl, which makes Linux system calls, instead of being reported
   missing. Every port links this way today. Relinked with musl kept off,
   none is missing anything, so none uses musl's code -- but its own
   missing-symbol count could not have shown that
   (known-issues D-SPIKES-LINK-ZIGS-MUSL-BEHIND-OUR-LIBC).
4. **zig's compiler runtime came first.** The first wrappers named all of
   zig's runtime ahead of our `libc.a`, and its `compiler_rt` carries weak
   copies of `memcpy`, `memset` and some sixty libm functions that ours
   defines too. ld.lld takes a symbol from the first archive on the line
   that defines it, so in a link so ordered those were zig's wherever ours
   was not already pulled in: GNU make's `sin`, and every port's 128-bit
   division helpers. zig's own driver puts `compiler_rt` last.

The link wrappers answer all four: they are compilers to cmake, so its own
checks link through them; their paths are written with `printf %q`; a link
goes to `ld.lld` with exactly the inputs named; and the libraries go in
zig's own order. `-Wl,-y` on opt's link shows where the symbols our libc
shares with zig's runtime come from: `memcpy`, `sin` and `__udivti3` from
our `libc.a`; `__cxa_throw` and `__gxx_personality_v0` from zig's
`libc++abi.a`, and `_Unwind_Resume` from its `libunwind.a` -- the real ones,
ahead of the stand-ins `posix/src/crt.rs` keeps for C++ linked with no C++
runtime.

## What configure finds, now that it asks our libc

Whatever a check links answers for this libc: `posix_spawn`, `futimens`,
`sysconf`, `sigaltstack`, `strerror_r`, the pthread mutexes and rwlocks, and
-- from zig's libunwind -- `__register_frame` and `_Unwind_Backtrace`, which
the musl run had answered "no". What a header decides is musl's headers', as
for every port (design-decisions §1011): `mallinfo`, `mallinfo2` and
`pthread_getname_np`, which our libc has, are not declared to LLVM's checks
by those headers, read as absent, and LLVM does without them.
