### BUG-OPENAT2-RESOLVE-NO-SYMLINKS-IGNORED. `openat2`'s `RESOLVE_NO_SYMLINKS` was accepted but never enforced — symlinks in the path were still followed — 2026-07-22 — ✅ RESOLVED 2026-07-22

**What:** `sys_openat2` (`kernel/src/syscall/linux.rs`) accepted the
`RESOLVE_NO_SYMLINKS` resolve bit and then forwarded to the normal openat
path with a comment claiming it was "trivially satisfied … no symlinks at
all". That claim is **false** — this kernel fully supports symlinks (memfs
*and* ext4). So `openat2(dirfd, path, {resolve: RESOLVE_NO_SYMLINKS})` would
silently follow symlinks in any path component, defeating the security
feature (RESOLVE_NO_SYMLINKS exists specifically to block symlink-swap
attacks). It is strictly stronger than `O_NOFOLLOW`: it must reject a
symlink in *any* component — parent or final — whereas O_NOFOLLOW guards
only the final one.

**Fix (2026-07-22):** Added `Vfs::resolve_no_symlinks` (kernel/src/fs/vfs.rs)
— a resolver mode that returns `TooManyLinks` (→ `ELOOP`) on encountering a
symlink in any component (threaded via a new `no_symlinks` param on
`resolve_inner`; dcache bypassed since it stores symlink-followed results).
Added `OpenFlags::NO_SYMLINKS` (bit 8) to kernel/src/fs/handle.rs; `open()`
uses the strict resolver when it is set. Threaded a `no_symlinks` bool from
`sys_openat2` → `sys_openat_ex` → `open_common`/`open_kernel_path_install`,
which OR the bit into the native OpenFlags (non-forgeable: a program can
only opt *into* stricter resolution). Handle self-test section 18 proves:
(a) parent symlink followed without the flag, (b) parent-component symlink →
ELOOP with NO_SYMLINKS (the case O_NOFOLLOW misses), (c) final symlink →
ELOOP, (d) symlink-free path still opens. NO_XDEV/NO_MAGICLINKS remain
genuinely trivial (no mid-walk bind mounts, no /proc magic links).
