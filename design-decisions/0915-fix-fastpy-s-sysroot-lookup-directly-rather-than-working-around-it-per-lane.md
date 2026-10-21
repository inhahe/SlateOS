## §915 — Fix fastpy's sysroot lookup directly rather than working around it per-lane

**Date:** 2026-09-07. **Decided by:** Operator. **Lane:** A.
**Status:** done 2026-10-21, in fastpy 0.3.6 (github.com/inhahe/fastpy
`5b11d88`), by lane D at the operator's request. `_find_slateos_sysroot_lib`
now tries `FASTPY_SLATEOS_SYSROOT` (an error unless it holds `libc.a`); on
SlateOS, the system's; the nearest directory at or above the working directory
that has `toolchain/build-sysroot.ps1` (an error unless its sysroot is built);
the same walk from fastpy's own directory; and last the sibling `os`. With that
and `scripts/ctest-fixtures.py` naming the worktree's sysroot for every build,
the boot test's sysroot identity check, which copied fastpy's old search and
could never see what a build linked, is gone (lane A, 2026-10-21).

**In short:** the C-test fixtures are compiled by fastpy (a separate project).
Fastpy hard-codes a sibling folder named `os` to find the C library, but the
project now lives in `os-lane-a`/`b`/`c` worktrees. The compiler never finds
the library, so every boot test prints a warning and the C-test results are
unattributable. The operator chose **option A**: fix fastpy directly (walk up
from launch directory, keep old `os` as fallback), bump its version, and
commit without pushing.

**Alternatives rejected:** (B) set the location explicitly per-lane — fixes
the symptom for one caller; (C) leave the warning — nothing degrades but the
gap persists.

**Implementation note:** this is a change to `D:\visual studio projects\fastpy`,
not to this tree. The fix is "walk up from the current working directory to
find the sysroot" with the old hard-coded `os` path as fallback.
