### TD-B-CHECK-LIBC-SHAPE-CANNOT-SEE-A-HAZARD-WHOSE-NAME-IS-NOT-ON-A-HAND-WRITTEN-LIST (lane B, 2026-08-21)

**What it is.** `scripts/check-libc-shape.py` asserts that our `libc.a` is
carved into archive members finely enough that a GNU program bringing its own
copy of a libc function can decline our copy. Its CHECK 3 (design-decisions.md
§348) states the correct property — a replaceable name must own its member, or
share only with other replaceable names — but it only applies that property to
names listed in the script's `REPLACEABLE` frozenset. A function we implement
that gnulib also defines, and that nobody adds to the list, is invisible to all
three checks.

**Where.** `scripts/check-libc-shape.py`, the `REPLACEABLE` set (~line 200) and
CHECK 3 (~line 420). `posix/src/string.rs`'s module header carries the standing
instruction to add new names, which is the current mitigation and is exactly as
reliable as a comment.

**How it bites.** Silently, and only at a port. The archive builds, every
`cargo test` passes, and the shape check prints OK — because the *tests* link
fixtures that do not bring competing definitions, which is structural rather
than an oversight (see the script's own header). The defect surfaces as a
duplicate-symbol error the first time a real GNU package is linked, i.e. at the
moment we are trying to port something. That is what happened with `wmempcpy`:
green for two months, then five coreutils binaries.

**How exposed we are, measured.** Not very, today. After §348's splits the
archive is clean and the eight known-to-matter names all own their members. The
list currently covers what four spikes (pkgconf, make, CPython, coreutils) have
actually observed, plus whole families rather than the specific member tripped.

**What the proper fix looks like.** Compute the set rather than curate it. The
generative rule is known and recorded in §348: gnulib compiles its replacement
as `rpl_foo` when the target *has* the function (harmless) and as plain `foo`
when it does not (collides), and `./configure` decides against the libc it was
configured with — zig's musl, for every spike here. So the hazard set is *the
names we export that musl does not*, which is a set difference over two archive
symbol indices, not a judgement call.

**Why it was not done that way.** It would make `toolchain/build-sysroot.ps1`
depend on having a musl `libc.a` to hand. That is true inside WSL and false on
the Windows side where the sysroot is actually built, and a check that cannot
run where the artifact is produced is not a check — the same reasoning that
keeps this script off `nm`. Two ways out, neither free: vendor a generated
`musl-exports.txt` beside the script and regenerate it when zig is upgraded
(cheap, but a second thing that can go stale); or move the shape check into the
spike harness where WSL is available (correct, but it stops being a *build*
gate, which is most of its value).

**Trigger to fix it properly:** if a fifth spike turns up a duplicate whose
name was not on the list. One such (`wmempcpy`) is a lesson; two is evidence
that curation does not work, and the vendored-export-list version should be
built at that point rather than debated.
