### B-ACCESS1. Linux-ABI `access`/`faccessat`/`faccessat2` always returned ENOENT (no-file skeleton-FS stub) — FIXED 2026-06-16

**Symptom:** Every `access`/`faccessat`/`faccessat2` call returned `-ENOENT`
unconditionally, even for files that exist in the VFS. The headline casualty
was unmodified GNU `make`: make issues `access("/bin/sh", X_OK)` **before**
spawning a recipe and, on failure, prints `"/bin/sh: No such file or directory"`
+ `Error 127` and never spawns the recipe shell — so no Makefile recipe could
run. (Confirmed via `strace` on real Linux: `access(shell, X_OK) = 0` precedes
the `clone3`.) Same class of stale stub as B-STAT1, but for the accessibility
probes rather than `stat`.

**Root cause:** `sys_access` / `sys_faccessat` / `sys_faccessat2` validated the
mode/flag bits and the path pointer, then hard-coded `linux_err(errno::ENOENT)`
with a comment that "without a backing filesystem there is no path that exists."
True when written; a silent lie once the VFS gained a backing store.

**Fix (proper):** The three syscalls now share a new `access_path_common`
back-end (kernel/src/syscall/linux.rs) that canonicalises the path against the
caller's cwd (`pcb::get_cwd`) and looks it up via `Vfs::metadata` (follow) /
`Vfs::lmetadata` (`AT_SYMLINK_NOFOLLOW`). Under the no-DAC capability model
(design-decisions §31) `F_OK`/`R_OK`/`X_OK` succeed for any existing file/dir —
consistent with `execve`, which ignores on-disk x-bits. Kernel context (no
caller PID) preserves the ENOENT no-file contract the fidelity self-tests
assert. Regression test: Path Z Part 34 (`self_test_linux_real_glibc_make`) runs
real GNU make end-to-end, whose recipe dispatch depends on `access(shell, X_OK)`.

**Known limitation (W_OK):** `W_OK` is granted for any existing file; it does
not yet consult per-mount read-only state (not tracked at this layer). A
read-only mount should return `EROFS` for `W_OK`. Low priority — no read-only
mounts are exposed to ring-3 writers today.
