## TD-D-FIXTURE-BUILD-RECIPES-ARE-COPIES — sixty-odd C fixtures each carry a copy of the same build recipe (lane D, 2026-10-17) — OPEN
**Status:** OPEN -- lane D's to do; deferred from round 205's review fixes because checking it means rebuilding every fixture, which the move to the new PC cut short.

**In short:** every C test fixture under `services/ctest-*/` has a
`build.py` that cross-compiles its `main.c` with `zig cc` and links it
against the C library with rust-lld. 44 of them are the same file but for
the fixture's name, and 18 more differ only in a flag or two. A change to
the recipe -- a compiler flag, the code model, the sysroot check -- must be
made in every copy, and a copy that is missed builds its fixture
differently from the rest without anyone noticing (Fable's review of lane
D's round 205's fixes).

**Where:** `services/ctest-*/build.py`, run by
`scripts/ctest-fixtures.py` and `scripts/create-ext4-rootfs.sh`.

**The proper fix:** one helper beside them -- `services/ctest_build.py`,
`build(here, name, extra_flags=())` -- holding the recipe, and each
`build.py` its docstring and one call to it, the 18 variants passing what
they add. Check: `python scripts/ctest-fixtures.py build` builds every
fixture, and each `.elf` is byte-identical to the one the old recipe made.
