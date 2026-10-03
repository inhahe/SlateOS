### BUG-OPENFLAGS-ENCODING. posix `translate_open_flags` emitted Linux `O_*` bits instead of native `OpenFlags` — 2026-07-21 — ✅ RESOLVED 2026-07-21

**What:** `posix/src/file.rs::translate_open_flags` produced a Linux-style
flag word for `SYS_FS_OPEN`'s third argument (access mode copied into the low
two bits as the POSIX enum 0/1/2, `O_CREAT`→bit 6 = 0x40, `O_TRUNC`→bit 9,
`O_APPEND`→bit 10, `O_EXCL`→bit 7). But the kernel's native `OpenFlags`
(kernel/src/fs/handle.rs) is an *independent single-bit layout*: READ=0x1,
WRITE=0x2, CREATE=0x4, TRUNCATE=0x8, APPEND=0x10, DIRECTORY=0x20. The kernel
did `OpenFlags::from_bits(raw)` directly — no re-translation — so the two
encodings had to match and didn't.

**Symptom:** `open(path, "w")` sent `O_WRONLY|O_CREAT|O_TRUNC` = `0x241`,
which the kernel decoded as READ (bit 0 set), *no* WRITE, *no* CREATE. For a
nonexistent file the create branch never ran → `NotFound`/ENOENT. Every
write-/append-mode open of a not-yet-existing file failed on-target. Caught by
the first pure-mode fastpy file-I/O self-test (`self_test_fastpy_slateos_fileio`):
`open('/tmp/fpyio.txt','w')` raised `FileNotFoundError` and the program exited 1
instead of 6. (This posix path had simply never been exercised on-target before:
prior file-touching self-tests staged their targets via kernel-side VFS calls,
not through the posix `open()` → `SYS_FS_OPEN` route.)

**Fix:** `translate_open_flags` now maps the POSIX access-mode enum to
independent READ/WRITE flags and each `O_*` to its native bit
(CREATE=0x4/TRUNCATE=0x8/APPEND=0x10/DIRECTORY=0x20). Unit tests in
`posix/src/file.rs` updated to assert the native encoding. Verified end-to-end
by the file-I/O boot self-test (exit 6).

**Residual limitation — ✅ RESOLVED 2026-07-22:** `O_EXCL` is now honoured.
`OpenFlags::EXCL` (bit 6, 0x40) was added to `kernel/src/fs/handle.rs`, and
`open()` returns `AlreadyExists` (→ `EEXIST`) when both `CREATE` and `EXCL` are
set and the path already exists — checked before the dir/regular split, so
exclusive create over an existing directory also fails with EEXIST. `EXCL`
without `CREATE` is ignored (matching Linux). `translate_open_flags`
(`posix/src/file.rs`) maps `O_EXCL` → `N_EXCL` (0x40). A false-pass-proof
handle self-test (section 16) asserts: (a) `CREATE|EXCL` over an existing file
→ AlreadyExists, (b) over a fresh path → succeeds and creates it, (c) a second
exclusive create → AlreadyExists, (d) `EXCL` without `CREATE` opens the file.

*Residual TOCTOU atomicity gap (narrow, tracked):* the exclusive check is
stat-based, and `open()` does the stat and the subsequent create in separate
VFS lock acquisitions (the `FileSystem` trait has no atomic create-exclusive
primitive). Two racers that both stat `NotFound` before either creates can both
proceed. The exists-at-open-time guarantee — the primary use of `O_EXCL` — holds
regardless; closing the race fully requires an atomic exclusive-create primitive
in the VFS/`FileSystem` trait across all backends (ext4, FAT32, tmpfs). Not
blocking; log an item if a caller depends on the race-proof guarantee.
