### [D] B-D-LANDLOCK-SAID-YES-THEN-NO — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/linux_landlock.rs`.

**In short:** Landlock lets a program lock itself out of files it does not
need -- a sandbox it sets up for itself. SlateOS's kernel does not enforce
it. A program asks first which Landlock version is there; ours answered
"version 1", and then refused every attempt to use it. So a program told
Landlock was available tried to sandbox itself and failed; a careful one
stopped rather than run unconfined. A kernel without Landlock answers the
first question "not supported", and the program carries on without it.

**What it was.** The module exported `landlock_create_ruleset`,
`landlock_add_rule` and `landlock_restrict_self` as C functions -- glibc has
none; programs make the three system calls through `syscall()` -- and the
version probe returned 1 while a real create answered `ENOSYS`, after
validators for arguments that could never be used.

**Fix.** The functions are gone, as the kernel-AIO ones went
(design-decisions.md §1114); the module is the header's constants and
structures. `syscall(SYS_landlock_*)` answers `ENOSYS` to all three, the
probe included -- "not supported by the current kernel", which every
Landlock-aware program tests for.

**What would change it:** a kernel that enforces Landlock (VFS and network
hooks, lane A's), at which point `syscall()` routes the three numbers to it.
