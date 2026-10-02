## §916 — `find -size 100` means 512-byte blocks (POSIX), not bytes

**Date:** 2026-09-07. **Decided by:** Operator. **Lane:** A.

**In short:** our `find -size 100` treated a bare number as bytes; every
other `find` treats it as 512-byte blocks (POSIX). The operator chose
**option C**: match POSIX, acknowledging that `b` for blocks is unintuitive
(most users expect bytes) but any deviation bites people familiar with the
tool, and there's no way to be both intuitive and compliant.

**Where it lives.** `userspace/shell/` — the `find` implementation's
`-size` parser.
