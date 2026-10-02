### BUG-NATIVE-MMAP-NUM. posix native memory-mgmt syscall numbers aliased kernel IRQ syscalls — 2026-07-21 — ✅ RESOLVED 2026-07-21

**What:** posix/src/syscall.rs numbered `SYS_MMAP=30`, `SYS_MUNMAP=31`,
`SYS_MPROTECT=32`, but the kernel's *native* table (kernel/src/syscall/
number.rs) has `SYS_MMAP=20`, `SYS_MUNMAP=21`, and 30/31/32 =
`SYS_IRQ_REGISTER`/`SYS_IRQ_WAIT`/`SYS_IRQ_RELEASE` (capability-gated). A
native `mmap()` therefore issued syscall 30 and hit the IRQ-register path,
which the syscall filter/cap check rejected with `PermissionDenied` (-400) —
*before* `sys_mmap` ran (hence no `[mmap-diag]` trace).

**Symptom:** The initiative-F fastpy self-test faulted with exit -8: the crt
`setup_main_thread_tls()` mmap returned -400, so `fs_base` was never set, and
the first `%fs:tpoff` access (fastpy's shadow-stack `__thread`, tpoff
-0x1b150) faulted at 0x14d20.

**Fix:** Corrected posix to the kernel's native numbers (MMAP=20, MUNMAP=21,
MPROTECT=22). Reserved 22 in kernel number.rs so it can't be reassigned.
See TD-NATIVE-MPROTECT below for the remaining native-mprotect handler work.
