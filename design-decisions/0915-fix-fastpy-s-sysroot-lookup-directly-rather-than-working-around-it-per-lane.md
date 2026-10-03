## §915 — Fix fastpy's sysroot lookup directly rather than working around it per-lane

**Date:** 2026-09-07. **Decided by:** Operator. **Lane:** A.

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
