### B-FS-HANDLE-PREAD-PWRITE-ARE-NOT-ATOMIC — 2026-08-13 — ✅ FIXED 2026-08-13 (`kernel/src/ipc/io_ring.rs`)

**Fix.** `exec_fh_pread` and `exec_fh_pwrite` now call
`fs::handle::read_at` / `fs::handle::write_at`. The seek sandwich is gone from
the kernel entirely (`grep -rn 'SeekFrom::Current(0)' kernel/` finds only the
handle self-test that legitimately *queries* the cursor).

The proper fix turned out to be **already three quarters built**: `read_at` and
`write_at` had existed in `kernel/src/fs/handle.rs` since the `pread64`/
`pwrite64` Linux-ABI work — they take `OPEN_FILES.lock()` once and index the
file without ever consulting `file.offset`, which is exactly the contract this
entry asked for. Every *syscall* path already used them
(`linux.rs:22995/24230/41027/41073`, `pcb.rs`'s mmap fault fill); only the
io_ring opcodes still had the hand-rolled emulation. So this was not missing
infrastructure, it was **one caller that never got migrated** — worth
remembering the next time an entry's "proper fix" reads like a large project:
check whether the primitive already exists under a different caller.

Two behaviours improved as a side effect:

- a `pread` on a *directory* handle now fails `IsADirectory` instead of
  seeking and then failing somewhere less specific;
- `pwrite` now ignores `O_APPEND`, which is what POSIX and Linux both
  specify ("the offset argument shall be used"). The old
  `seek`-then-`handle::write` path let an `O_APPEND` handle silently redirect
  the write to end-of-file, discarding the caller's offset.

**Regression test.** `test_fh_positioned_io_leaves_the_cursor_alone` in
`io_ring.rs` (run from both `self_test` and the post-mount `self_test_fh`).
It writes a 26-byte self-identifying file (`A`..`Z`), reads 4 bytes
sequentially, then interleaves a `PREAD` at offset 20 and a `PWRITE` at offset
10, asserting after each that `seek(Current(0))` is still 4 — and finally that
the next sequential read yields `EFGH`, i.e. the stream never noticed the
positioned I/O at all. That last assertion is precisely what the seek sandwich
could not offer a concurrent peer.

**The original report follows.**

**What.** `IO_OP_FH_PREAD` and `IO_OP_FH_PWRITE` are emulated as
`seek(Current(0))` → `seek(Start(offset))` → `read`/`write` →
`seek(Start(saved))`. The whole sequence is not atomic with respect to the file
handle's cursor.

**Impact.** A file handle can be shared between threads (and, after
`SYS_PROCESS_SET_EXEC_FDS`, between processes). If a peer touches the same
handle inside that window, the results interleave arbitrarily:

- the peer's `read` starts from `offset` instead of its own position;
- the restore at the end clobbers a position the peer legitimately advanced;
- two concurrent preads on one handle can each restore the *other's* saved
  position, leaving the cursor somewhere neither of them was.

The whole point of `pread`/`pwrite` (POSIX) is to be a positioned I/O that does
**not** disturb the cursor and is safe to issue concurrently on a shared
descriptor. This implementation provides neither guarantee.

**Repro sketch.** Two threads sharing one `fh`: thread A submits
`IO_OP_FH_PREAD` at offset 0 in a loop; thread B does sequential
`IO_OP_FH_READ`s. B's stream will show repeats/gaps as A's restore lands
between B's reads.

**Proper fix.** Give `fs::handle` real positioned operations —
`read_at(fh, offset, buf)` / `write_at(fh, offset, buf)` — that take the
handle's lock once and index the file without touching the stored cursor at
all, and have the io_ring opcodes (and any future `pread`/`pwrite` syscalls)
call those. The seek-sandwich should not exist anywhere. Until then the io_ring
opcodes at least report the restore failure instead of discarding it.
