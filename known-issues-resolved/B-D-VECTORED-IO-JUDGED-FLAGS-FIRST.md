### [D] B-D-VECTORED-IO-JUDGED-FLAGS-FIRST — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/file.rs`: `readv`, `writev`, `preadv`, `pwritev`,
`preadv2`, `pwritev2`, and `plan_rw_flags`, which kernel AIO shares.

**What it was.** `preadv2` and `pwritev2` judged their `RWF_*` flags before
the descriptor, where fs/read_write.c judges them last -- so a bad
descriptor with a bad flag said `EINVAL`, and a transfer of nothing was
refused over a flag Linux never looks at. The flags themselves were not
Linux's: an unknown bit was `EINVAL` (Linux: `EOPNOTSUPP`), `RWF_DSYNC` and
`RWF_SYNC` on a read were refused (Linux accepts them and does nothing), and
`RWF_APPEND` was refused (Linux writes at the end). The four older calls
checked their vector with a copy of `iovec_from_user` that skipped a
segment length past `SSIZE_MAX` (`EINVAL`), `access_ok` (`EFAULT`) and the
`MAX_RW_COUNT` cap.

**Fix.** One engine for the six, in `do_readv`/`do_preadv`'s order; the
vector imported by `uio.rs`; the flags as Linux 6.6's `kiocb_set_rw_flags`
takes them, with `do_loop_readv_writev`'s stricter rule for a descriptor
with no `read_iter`/`write_iter`; `RWF_APPEND` honoured.

**What remains.** Linux judges `FMODE_READ`/`FMODE_WRITE` after the vector;
here the transfer does, because only descriptors `open` made record their
access mode reliably -- so a zero-length vector on a descriptor open the
other way is 0 here where Linux says `EBADF`. `RWF_NOWAIT` is the `EAGAIN` of
a transfer not attempted: correct for a caller that falls back, and a busy
loop for one that polls and then insists on `RWF_NOWAIT` (none known).
