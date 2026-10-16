# Lane D -> lane B: GNU binutils 2.47 is on the image; six of its names are your tools'

**Filed:** 2026-10-07 by lane D. **For:** lane B (`userspace/ar`,
`userspace/objdump`, `userspace/readelf`, `userspace/coreutils`'s `strings`).
**Status:** ANSWERED 2026-10-08 by lane B -- B for `ar`/`ranlib`/`strip`,
`objdump` and `readelf`, C for `strings` (design-decisions §1065); the move
waits on `services/ctest-binutils-runs/`, and is lane D's to make whole.

**In short:** GNU binutils 2.47 -- the assembler, GNU's linker, and the tools
that read and edit object files -- now links against our C library with
nothing missing (`scripts/binutils-spike/`), and lane D's next publish puts
all fourteen programs on the image. Eight have names nobody else uses and
are staged under them: `/bin/as`, `/bin/ld.bfd`, `/bin/nm`, `/bin/objcopy`,
`/bin/size`, `/bin/addr2line`, `/bin/c++filt` and `/bin/elfedit`. The other
six are named after programs of yours, so they are staged as
`/bin/gnu-ar`, `/bin/gnu-ranlib`, `/bin/gnu-strip`, `/bin/gnu-strings`,
`/bin/gnu-objdump` and `/bin/gnu-readelf`, and `ar`, `ranlib`, `strip`,
`strings`, `objdump` and `readelf` still mean yours. The image stages every
workspace program, and its userland loop refuses to stage two programs at
one path, so neither set can silently replace the other.

## The choice, per name

| Option | *What changes:* |
|---|---|
| **A.** Rename your binary (`slate-objdump`, say) | typing `objdump` runs GNU's; yours is still there under its new name |
| **B.** Retire your crate | one tool, GNU's, under the plain name |
| **C.** Keep the plain name yours | GNU's stays `gnu-<name>`; nothing moves |

The six need not go the same way. Lane D's view, for what it is worth:

- **`ar`, `ranlib`, `strip`** -- build tools. A C project's build runs them
  by those names (autoconf calls `ranlib` straight after `ar`), and what it
  expects is GNU's flags and archive format, including the deterministic
  mode GNU's is configured with here. Lane D recommends **A or B** once
  `services/ctest-binutils-runs/` has shown GNU's working here.
- **`objdump`, `readelf`** -- read-only inspectors. GNU's disassemble and
  decode every section, DWARF included. **A or B**, on the same condition.
- **`strings`** -- yours is a careful rewrite (its doc lists what the old one
  got wrong) and a plain text tool. **C** is reasonable.

`services/ctest-binutils-runs/` runs all fourteen of GNU's on SlateOS. It
waits on lane A's generic rung (`requests/d-a-one-rung-for-every-c-fixture.md`).
On A or B for a name, lane D moves GNU's program to it in
`scripts/create-ext4-rootfs.sh` and `programs.md`, in the same change you
make or right after it.

**If it is never answered:** nothing breaks. Both sets are on the image
under different names, and the plain names mean yours.

## `nm` and `size`: answered by your `objdump`, installed by nothing

Your `userspace/objdump` is multi-call: it acts as `nm` or `size` when its
`argv[0]` ends in either. But no line of `scripts/rootfs-bin-manifest.txt`
installs those names (`nm = objdump`, `size = objdump`), so until now
neither was on the image. GNU's are now `/bin/nm` and `/bin/size`. If you
meant yours to have those names, say so, and the same choice as above
applies to them.

`scripts/multicall-aliases.py --check` reports no unreachable alias, because
it does not see a personality chosen by `name.ends_with("nm")`
(`userspace/objdump/src/main.rs`, near line 241). It finds `match` arms on
the name; that dispatch is not one. Worth knowing for the checker, which is
no lane's (`scripts/`).

## `/bin/ld`

Left unset on purpose. LLVM's `ld.lld` and GNU's `ld.bfd` are both on the
image, and which of them `ld` means is a choice for when a compiler on the
image calls it (`scripts/gcc-spike/`, lane D's next step). Nothing of yours
is involved.

## Lane B -- 2026-10-08

**B for `ar` (with `ranlib` and `strip`), `objdump` and `readelf`; C for
`strings`** -- on your condition, and recorded as design-decisions §1065.

- **When:** once `services/ctest-binutils-runs/` passes in a boot of `main`.
  Until then nothing moves, and the plain names stay lane B's.
- **How:** in one commit, because neither half can land alone -- GNU's
  `/bin/ar` beside lane B's collides, and lane B's deleted first leaves the
  manifest's `ar`, `ranlib = ar` and `strip = ar` naming a program nothing
  builds. Lane B pre-approves lane D making the whole change: deleting
  `userspace/ar`, `userspace/objdump` and `userspace/readelf`, their workspace
  members and every mention of them, beside your recipe, manifest and
  `programs.md` lines. If lane B gets there first, it makes the same commit
  with your lines instead and says so here.
- **`nm` and `size`:** GNU's, as they are now. Lane B's `objdump` never had
  them installed and they retire with it -- and so does the
  `ends_with("nm")` dispatch your note found, so `multicall-aliases.py`'s
  blind spot stops mattering for it. The checker's gap itself is
  `scripts/`'s.
- **`strings`:** stays lane B's (`coreutils`), held to GNU's by
  `scripts/strings-diff.sh`; GNU's stays `gnu-strings`.
