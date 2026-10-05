### B-CWD1. Linux-ABI relative path resolution ignored the per-process cwd (relative `open`/`*at` resolved against `/`) — FIXED 2026-06-16

**Symptom:** After a process did `chdir("/dir")`, a relative `open("file")`
(or `openat(AT_FDCWD, "file")`, and the relative-path branches of stat,
access, mkdir, unlink, rename, readlink, chmod, chown, etc.) resolved the
path against the filesystem **root** rather than `/dir`. e.g. `cd /reltest &&
echo x > rel.txt` created `/rel.txt`, not `/reltest/rel.txt`. This broke
standard Unix semantics for essentially every program that uses relative
paths after changing directory.

**Root cause:** The Linux ABI's `open_common` forwarded the raw userspace
path pointer straight to `sys_fs_open` → `fs::handle::open` →
`Vfs::resolve_path`, and `resolve_at_path` (the `*at` family helper) returned
the path verbatim for the `AT_FDCWD`/relative case. None of those layers
take a PID, and `Vfs::normalize_path` treats `rel` identically to `/rel`
(it splits on `/` and always re-emits a leading slash), so the per-process
cwd stored in the PCB by `chdir` (`pcb::set_cwd`) was never consulted on the
open side. The limitation was even documented in `resolve_at_path`'s doc
comment ("there is no per-process cwd in the native path resolver").

**Fix (proper):** Resolve relative paths against the caller's cwd at the
Linux ABI boundary, reusing the existing `canonicalize_path(cwd, path)`
helper (already used by the chroot gate and `fstatat`). `open_common`
(kernel/src/syscall/linux.rs) now canonicalises the path against
`pcb::get_cwd(caller)` and opens via a new `handlers::fs_open_kernel_path`
(a kernel-string variant of `sys_fs_open` that does the File-READ cap check
+ handle registration without reading userspace), and `resolve_at_path`
canonicalises its `AT_FDCWD`/absolute result the same way. Kernel context
(no caller PID) falls back to cwd `"/"`, preserving the prior behaviour for
in-kernel callers and the native ABI (`sys_fs_open` is untouched). Absolute
paths are normalised but otherwise unchanged. Regression test: Path Z
Part 23 (`self_test_linux_real_glibc_shell_relpath`) runs `cd /reltest &&
echo RELOK > relfile.txt` in ring 3 and asserts the file landed at
`/reltest/relfile.txt` and **not** at `/relfile.txt`.
