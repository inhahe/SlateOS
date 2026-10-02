### [D] B-D-IOPRIO-CHECK-WAS-PRE-6-5 — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/process.rs`, `ioprio_check_cap` (new) and `ioprio_set`.

**What it was.** `ioprio_set` judged an I/O priority by the rule Linux
dropped in 6.5: all thirteen data bits as the level, so a priority carrying a
hint (bits 3-12, `IOPRIO_PRIO_HINT`) was `EINVAL`, and a class field above
three bits was a class nothing matched. Linux 6.6's `ioprio_check_cap` masks
the class to three bits and reads only the low three as the level.

**Fix.** `ioprio_check_cap`, 6.6's, shared by `ioprio_set` and kernel AIO's
`IOCB_FLAG_IOPRIO`.
