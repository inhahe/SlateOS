## D-POSIX-F-GETFL-NEVER-REPORTS-O-LARGEFILE — `fcntl(F_GETFL)` leaves out the `O_LARGEFILE` bit Linux's 64-bit kernel adds to every `open` (lane D, 2026-10-16) — OPEN
**Status:** OPEN

**In short:** on x86-64 Linux, every descriptor `open` makes -- a file's, a
named pipe's -- reports bit `0o100000` (`O_LARGEFILE`, the kernel's flag
that the file may be larger than 2 GiB) in `fcntl(fd, F_GETFL)`, though
glibc's `<fcntl.h>` names `O_LARGEFILE` 0 there. This library reports only
the flags it stores, so `F_GETFL` answers `0o4000` where Linux answers
`0o104000` for a file opened `O_RDONLY | O_NONBLOCK`. A program that masks
the answer with the flags it means (`O_ACCMODE`, `O_NONBLOCK`, `O_APPEND`),
as nearly every program does, sees no difference; one that compares the
whole word with a value it built -- a test suite, a tracer printing it --
does. Found writing `ctest-fifo`, which masks the word for that reason.

**Where:** `posix/src/fcntl_ops.rs` (`F_GETFL`), and the opens that store
the flags (`posix/src/file.rs`: `open_resolved`, `open_tmpfile`,
`open_fifo`, `openat2_forward_inner`).

**The proper fix:** store the kernel's bit with the flags of every
descriptor an `open` makes, as Linux's `force_o_largefile` does -- not
those `pipe`, `socket`, `eventfd` and kin make, which Linux's `F_GETFL`
answers without it -- with a test that checks each kind's word against
Linux's whole, and `F_SETFL` leaving the bit alone, as Linux's does.
