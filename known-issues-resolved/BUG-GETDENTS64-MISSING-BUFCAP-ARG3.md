### BUG-GETDENTS64-MISSING-BUFCAP-ARG3. The raw `getdents64` had the same missing-capacity bug `opendir` was fixed for in July, and returned an empty directory for every fd — 2026-09-01 — ✅ RESOLVED 2026-09-01

**What it was.** `posix::dirent::getdents64` snapshotted the directory with

```rust
syscall3(SYS_FS_LIST_DIR, path_ptr, path_len, buf_ptr)
```

`SYS_FS_LIST_DIR` (603) reads the buffer's capacity from **arg3** and computes
`max_entries = capacity / entry_size`. Passing the buffer pointer as arg3 and
nothing as the capacity meant the kernel was told the buffer held zero entries,
so it filled none and answered `0` — which `getdents64` reported to the caller
as an empty directory. Not an error, not a short read: a directory with no files
in it, for every fd, always.

**Why it is the same bug twice.** This is
`BUG-OPENDIR-MISSING-BUFCAP-ARG3` (2026-07-22, above) in a second call site.
That one was found because `ls` uses `opendir`, so it was visible the moment
anyone listed a directory. This one was not found for six weeks because **nothing
in-tree calls the raw syscall wrapper** — our coreutils go through `opendir`, and
`getdents64` exists for foreign programs (Go and Rust runtimes, `ls -f`
implementations, `busybox`) that bypass libc's dir streams. A wrapper with no
in-tree caller gets no in-tree coverage; the fix for the first instance came with
a regression test on `opendir`, and there was no equivalent test here to fail.

**How it was fixed.** Not by adding the missing argument. `getdents64` now takes
its snapshot through the descriptor with `SYS_FS_GETDENTS_PINNED` (664), whose
ABI has the capacity in the argument it is documented to be in and which reports
the size of the *complete* listing so a short buffer is retried rather than
silently truncated. The path-based call is gone from this file entirely, which is
what makes a third instance of this mistake impossible rather than merely
unlikely.

**What now covers it.** `fill_dirent64_batch` is a pure function over the
kernel's packed records and the caller's buffer, and is unit-tested on the host —
including the batching boundary, the skip cases, and that `d_ino` is the
filesystem's inode rather than the entry's index. The syscall itself still cannot
be exercised on the host (it answers `ENOSYS`), which is exactly why the decoding
was factored out of it.
