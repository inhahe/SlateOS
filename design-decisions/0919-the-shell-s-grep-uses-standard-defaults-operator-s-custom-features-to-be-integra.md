## §919 — The shell's `grep` uses standard defaults; operator's custom features to be integrated

**Date:** 2026-09-07. **Decided by:** Operator. **Lane:** A.

**In short:** our shell `grep` defaulted to case-insensitive search with
line numbers, unlike every other Unix `grep`. The operator chose **option A**:
match standard defaults (case-sensitive, no line numbers unless `-n`). The
operator also noted a custom grep implementation at
`D:\visual studio projects\grep` (Python and C++) with additional features
beyond GNU grep, and wants those integrated into SlateOS's grep so it
becomes a superset. The grep implementation is in `userspace/` (lane B's
territory), so lane B handles the actual port.
