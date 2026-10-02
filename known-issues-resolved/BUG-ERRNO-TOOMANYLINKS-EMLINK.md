### BUG-ERRNO-TOOMANYLINKS-EMLINK. native `errno::translate` mapped the kernel symlink-loop error to `EMLINK` instead of `ELOOP` — 2026-07-22 — ✅ RESOLVED 2026-07-22

**What:** `posix/src/errno.rs` mapped `native::TOO_MANY_LINKS` (-506) → `EMLINK`,
with a test asserting that and a comment claiming -506 was the hard-link-count
limit. But `KernelError::TooManyLinks`'s own doc/message is "too many symbolic
links", and *every* kernel producer of it is symlink-loop / max-symlink-depth
semantics (symlink-resolution depth guards in vfs/memfs/ext4, circular-symlink
detection, and now the O_NOFOLLOW final-symlink guard) — no kernel path
produces a hard-link-count EMLINK. So a native program hitting a circular
symlink got `EMLINK` ("too many links") instead of the correct `ELOOP` ("too
many levels of symbolic links"). The Linux-ABI translation (linux.rs:1346)
already mapped it correctly to ELOOP; only the native path was wrong.

**Fix (2026-07-22):** native `errno::translate` now maps `TOO_MANY_LINKS` →
`ELOOP`, unifying both ABIs. Updated the test + comments. (If a real
hard-link-count limit is ever enforced it must use a *distinct* kernel error
code, not this symlink error — noted in the errno.rs comment.)
