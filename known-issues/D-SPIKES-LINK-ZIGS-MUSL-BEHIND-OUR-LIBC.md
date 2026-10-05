## D-SPIKES-LINK-ZIGS-MUSL-BEHIND-OUR-LIBC — every port's final link has zig's musl `libc.a` on it, after ours, so a function our libc lacks would be supplied by musl instead of failing the link (lane D, 2026-10-01)

**Status:** OPEN — latent: no port binary uses musl's code, measured; the fix is a direct ld.lld link for every port, as the LLVM port's.

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
