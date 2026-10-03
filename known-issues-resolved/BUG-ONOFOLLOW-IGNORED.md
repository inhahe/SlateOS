### BUG-ONOFOLLOW-IGNORED. posix `O_NOFOLLOW` was accepted and stored but never enforced — `open()` silently followed a final symlink — 2026-07-22 — ✅ RESOLVED 2026-07-22

**What:** `translate_open_flags` (posix/src/file.rs) dropped `O_NOFOLLOW`, and
the kernel `open()` (`kernel/src/fs/handle.rs`) unconditionally resolved
symlinks via `Vfs::resolve_path` (which follows the final component). So
`open(path, O_NOFOLLOW)` on a symlink silently opened the *target* instead of
failing with `ELOOP` — a POSIX-correctness gap and a mild security concern
(a caller trying to avoid symlink attacks was not actually protected).

**Fix (2026-07-22):** Added `OpenFlags::NOFOLLOW` (bit 7, 0x80) to
`kernel/src/fs/handle.rs`. `open()` now runs a NOFOLLOW guard *before*
`resolve_path`: it `Vfs::lstat`s the path (lstat does not follow the final
component) and returns `TooManyLinks` (→ `ELOOP`) if the last component is a
`Symlink`; parent-component symlinks are still followed (POSIX). Non-symlink
or not-yet-existing final components fall through to the normal open/create
path. `translate_open_flags` maps `O_NOFOLLOW` → `N_NOFOLLOW` (0x80). A handle
self-test (section 17) proves: (a) without NOFOLLOW a symlink resolves to its
target, (b) with NOFOLLOW a final symlink → TooManyLinks, (c) NOFOLLOW on a
non-symlink opens normally.

**Linux-ABI parity (2026-07-22):** The kernel-side Linux translator
(`kernel/src/syscall/linux.rs::translate_open_flags`, ~5352) dropped both
`O_EXCL` and `O_NOFOLLOW`, so glibc/Linux-ABI programs got neither guarantee
even though `sys_openat`/`open_kernel_path_install` funnel through the same
`fs::handle::open`. Added `oflags::O_NOFOLLOW` (0o400_000) and now map
`O_EXCL → OpenFlags::EXCL` and `O_NOFOLLOW → OpenFlags::NOFOLLOW`, so the shared
`handle::open` guard fires identically for both ABIs.
