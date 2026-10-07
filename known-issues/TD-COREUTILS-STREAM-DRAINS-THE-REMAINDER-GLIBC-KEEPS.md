## TD-COREUTILS-STREAM-DRAINS-THE-REMAINDER-GLIBC-KEEPS (lane B, 2026-10-07)

**Status:** FIXED 2026-10-07 (lane B), pending a boot test on `main` before
the move to `known-issues-resolved/`. `stdfd::Stream` now writes by glibc's
own arithmetic, the one copy of it in `coreutils::stdio::xsputn`, which
`StdioFile` uses too.

**In short:** `stdfd::Stream`, the buffered standard output most utilities
here write through, emptied its whole buffer as soon as it held 4096 bytes.
glibc's stdio, which upstream's programs write through, does not: it writes
the buffer only when a write finds it full, writes whole blocks of a long
write directly, keeps the remainder, and drops the rest of a write whose
flush failed. Every byte arrives either way when the disk has room. What
differs is what is still waiting when the stream is closed, and that decides
a sentence: after a full disk, gnulib's `close_stdout` says `write error: No
space left on device` when its close still had something to write, and a
bare `write error` when it did not.

**Where:** `userspace/coreutils/src/stdfd.rs`, `Inner::put`, which had its
own rules: `Buffering::Block` appended everything and drained everything once
the buffer reached 4096; `Buffering::Line` drained through the last newline
and kept the tail even when that drain failed. It now calls
`coreutils::stdio::xsputn`, with the buffer's size chosen at the first write
from the descriptor's `st_blksize` (`coreutils::stdio::buffer_size`), as
`_IO_file_doallocate` chooses it.

**What it changed, measured:** a C program doing the same `fwrite`s onto
`/dev/full` under glibc 2.39, against the old `Stream`:

| writes | glibc holds at the close | glibc's last word | the old `Stream` |
|---|---|---|---|
| one of 5000 bytes | 0 | `write error` | the same |
| 50 of 100 bytes | 900 | `write error: No space left on device` | the same |
| 4 of 1024 bytes (the buffer exactly full) | 4096 | with the reason | `write error`: it drained at exactly 4096 |
| 5 of 1024 bytes | 0 | `write error`: the fifth write's flush failed and took the write with it | with the reason: it kept the fifth |
| line-buffered, `x\nxxx` | 0 | `write error`: `xxx` went with the failed line | with the reason: it kept `xxx` |
| line-buffered, 5000 bytes and no newline | 0 | `write error` | with the reason: it held them all |

and on a disk that fills part-way, the old rule wrote out the remainder glibc
keeps for the close, so a close that glibc would have failed, with a reason,
had nothing to write. `stdfd.rs`'s `glibc_measured` test holds the table.

**What this was first thought to be, and was not:** `date --help >/dev/full`
said `date: write error` where GNU's said it with the reason, and this entry
blamed the drain. It was the help being written in one piece: a single write
of more than a buffer onto a full disk leaves nothing held under glibc's
rules too (the first row above). GNU's `usage` writes a paragraph per
`fputs`, so the last paragraphs are still held at the close; `date` was
changed on 2026-10-07 to write its help in the same pieces, which is the fix
for that case under either rule.
