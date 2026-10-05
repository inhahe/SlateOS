### BUG-OPENDIR-MISSING-BUFCAP-ARG3. posix `opendir`/`fdopendir` issued `SYS_FS_LIST_DIR` via `syscall3`, omitting the buffer capacity the kernel reads from arg3 → kernel computed `max_entries = 0` → every directory listing came back empty — 2026-07-22 — ✅ RESOLVED 2026-07-22

**What:** `opendir` (posix/src/dirent.rs:126) and `fdopendir` (~:608) called
`syscall3(SYS_FS_LIST_DIR, path_ptr, path_len, buf_ptr)`, never passing the
buffer's capacity. But kernel `sys_fs_list_dir` (kernel/src/syscall/handlers.rs:5690)
reads `buf_cap = args.arg3` and computes `max_entries = buf_cap / FS_DIR_ENTRY_SIZE`.
With arg3 defaulting to 0, `max_entries = 0`, so the kernel wrote no entries and
returned a count of 0 regardless of the directory's real contents.

**Symptom / repro:** The new `fastpy-ls` tool (`ls <dir>` via `os.listdir` →
native `opendir`/`readdir` → `SYS_FS_LIST_DIR`) is the *first* on-target caller
of native `opendir`, so it first exposed this latent ABI mismatch. Its self-test
staged `/tmp/lsdir` with three files; a kernel-side `Vfs::readdir` diagnostic saw
all 3, but the ring-3 process printed none and exited 0 (an empty-listing
false-pass). Every prior directory read had gone through other paths, so the bug
had never fired.

**Fix:** Pass the buffer capacity as arg3 via `syscall4(..., dir.buf.len())` in
both `opendir` and `fdopendir`. Additionally hardened the `fastpy-ls` self-test:
the tool now `sys.exit(count)` and the kernel test asserts `exit_code == 3` (the
entry count) rather than merely exit 0, so an empty listing can never false-pass
again.
