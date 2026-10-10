## D-SPIKES-LINK-ZIGS-MUSL-BEHIND-OUR-LIBC — every port's final link has zig's musl `libc.a` on it, after ours, so a function our libc lacks would be supplied by musl instead of failing the link (lane D, 2026-10-01)

**Status:** FIXED 2026-10-05

**In short:** the programs ported to SlateOS -- bash, make, pkgconf, CMake,
CPython, espeak-ng -- are linked against SlateOS's own C library. Measured
on 2026-10-01, the tool that links them, zig's `cc`, also puts zig's copy of
musl (Linux's C library) on every link, whatever it is told. Ours comes
first, so musl only fills gaps: a function ours lacks would be taken from
musl rather than reported missing. musl's functions call Linux's system
calls, which on SlateOS are other calls, so such a program would misbehave
at that call rather than fail to build. Today none does: a relink with
musl kept off finds nothing missing.

**What was measured** (zig 0.13.0, the one `scripts/lib/worktree.sh` pins):
`zig cc -v` with `-static -nostdlib`, `-nodefaultlibs`, `-nostdlib++`, or
any mix, links `.../zig/o/<hash>/libc.a` -- zig's musl -- after every input
it was given. `zig ld.lld` links exactly its inputs.

**Why no binary is affected today -- measured, not inferred:** our `libc.a`
is given twice before zig appends musl, so every symbol it defines is ours,
and musl's are taken only for what it lacks. Linking each port's own
objects again through zig's `ld.lld`, with exactly its inputs and no musl,
reports 0 undefined symbols for make, pkgconf, eSpeak NG, CPython and CMake
(2026-10-01): musl supplied nothing to any of them. bash, linked so, has
nothing undefined either; it fails on duplicate symbols instead, a matter of
the link's order, not of musl. coreutils' tree is not on this machine. Lane
B checked its Oils port by the same property: of the symbols the ELF
defines, none is one musl's `libc.a` defines and ours does not. A scan for
musl's internal names alone would not have shown it: a self-contained musl
function pulled in for a gap uses none of them.

**Why it matters anyway:** each port's `MISSING_COUNT=0` is the proof that
our libc has everything the program calls -- and with musl behind ours, a
missing function never counts as missing. The proof is unsound as it
stands, even where its answer happens to be right.

**Where:** the final links in `scripts/cmake-spike/run.sh`,
`scripts/make-spike/run.sh`, `scripts/bash-spike/slatelink.sh`,
`scripts/pkgconf-spike/run.sh`, `scripts/cpython-spike/slatelink.sh`,
`scripts/espeak-spike/run.sh` and `slatelink.sh`, and
`scripts/coreutils-spike/run.sh` -- each `"$SLATE_CC"` or `"$SLATE_CXX"`
with `-static -nostdlib`, each with a comment saying that this keeps zig's
musl out. The coreutils port goes further: it strips `-lpthread`, `-lrt` and
the rest, believing them the way musl got in, and its
`LINKS_THAT_PULLED_ZIG_MUSL` counts the link logs that name zig's cache. A
log names it only when musl's member collides with ours (a duplicate), so a
missing function musl filled in silently is not counted there either.

**The proper fix:** link with `zig ld.lld` and the inputs named, as
`scripts/llvm-spike/run.sh` does (2026-10-01): `slate_make_link_wrappers` in
`scripts/lib/worktree.sh` writes compiler wrappers that hand a compile to
zig's driver and a link to ld.lld, with zig's C++ runtime, our `libc.a` and
zig's compiler runtime after the build's inputs, in zig's own order. Every
port's final link through that helper, and each port relinked and its
counts read again. The order is part of the fix: the CMake port's link
named zig's runtime ahead of our `libc.a`, and `compiler_rt` carries weak
copies of `memcpy`, the 128-bit division helpers and some sixty libm
functions, so where ours had not already been pulled in, CMake got zig's.

**Fixed (2026-10-05):** every port's final link goes through
`slate_make_link_wrappers` now -- make, pkgconf, bash, eSpeak NG (both its
scripts), CPython, CMake and coreutils -- so each links exactly the inputs
it names, with zig's C++ runtime ahead of our `libc.a` and zig's
`compiler_rt` behind it, in zig's own order. Relinked so, make, pkgconf,
eSpeak NG, CPython and CMake have nothing undefined and nothing duplicated,
as they had with musl behind them. CMake's link, run again with
`--why-extract`, takes nothing at all from `compiler_rt`: the 128-bit
division helpers our own members call (`__udivti3`, `__divti3`,
`__umodti3`) come from our `libc.a`. bash, linked in its own order for the
first time, needed our library to let it bring its own `getenv` family
(D-POSIX-GETENV-AND-GETCWD-COULD-NOT-BE-REPLACED), and links with nothing
undefined or duplicated. coreutils 9.5, built again from source, links all
107 of its programs the same way, with nothing undefined or duplicated.
Its `LINKS_THAT_PULLED_ZIG_MUSL` is gone: the wrapper links no musl, and
every binary is checked for the SlateOS note instead
(`BINARIES_WITHOUT_SLATEOS_NOTE`, 0 of 107).

**Oils followed on 2026-10-05**, when lane D staged it
(`requests/b-ad-genuine-oils-staged-and-run-at-boot.md`). Lane B's port, it
had arrived on 2026-10-01 linking through zig's `c++` driver with `-nostdlib`
-- the last link in the tree that did. It links through
`slate_make_link_wrappers` now, in its new `scripts/oils-spike/slatelink.sh`:
nothing undefined, nothing duplicated, as before.
