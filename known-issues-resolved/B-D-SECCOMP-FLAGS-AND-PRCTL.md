### [D] B-D-SECCOMP-FLAGS-AND-PRCTL — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/linux_seccomp.rs` (`check_set_mode_filter`, new);
`posix/src/unistd.rs` (`prctl`).

**What it was.** `seccomp(SECCOMP_SET_MODE_FILTER)` refused `TSYNC` with
`NEW_LISTENER` even alongside `TSYNC_ESRCH` -- the flag that exists to make
that pair unambiguous, which Linux accepts -- and refused `TSYNC_ESRCH`
without `TSYNC`, a rule Linux 6.6 does not have. It never read the program
header it was given, so a zero-length program reached the privilege gate and
a header with no program answered `ENOSYS`. And `prctl(PR_GET_SECCOMP)` and
`prctl(PR_SET_SECCOMP)` answered `EINVAL`: a sandbox asking whether it is
already confined was told the call does not exist.

**Fix.** Linux 6.6's order: the flags, the header (`EFAULT`), its length
(`EINVAL`), the gate (`EACCES`), the program pointer (`EINVAL`), then
`ENOSYS` as before. `PR_GET_SECCOMP` answers the mode, disabled;
`PR_SET_SECCOMP` is `seccomp()` by `prctl_set_seccomp`'s mapping.
