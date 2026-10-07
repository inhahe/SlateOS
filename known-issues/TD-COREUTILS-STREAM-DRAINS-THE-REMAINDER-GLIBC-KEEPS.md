## TD-COREUTILS-STREAM-DRAINS-THE-REMAINDER-GLIBC-KEEPS (lane B, 2026-10-07)

**Status:** OPEN (lane B).

**In short:** `stdfd::Stream`, the buffered standard output most utilities
here write through, empties its whole buffer as soon as it holds 4096 bytes.
glibc's stdio, which upstream's programs write through, writes out only the
full blocks and keeps the remainder. Every byte arrives either way; what
differs is whether anything is still waiting when the stream is closed, and
that decides a sentence. After a full disk, upstream's last word is `write
error: No space left on device` when its close still had something to write,
and a bare `write error` when it did not. A program here that writes more
than a buffer in one go can therefore end with the bare one where GNU's ends
with the reason.

**Where:** `userspace/coreutils/src/stdfd.rs`, `Inner::put`, the
`Buffering::Block` arm: it appends everything and drains everything once the
buffer reaches `BUFFER`.

**Reproduce:** a single write of more than 4096 bytes to `/dev/full` through a
`Stream`. `date --help >/dev/full` was the case found, on 2026-10-07: GNU
`date: write error: No space left on device`, ours `date: write error`. `date`
now writes its help in pieces, as `usage` does with one `fputs` a paragraph,
which leaves the end of it waiting under either rule -- so the harness no
longer shows it, and the general rule is unchanged.

**The fix:** make `Buffering::Block` follow `_IO_new_file_xsputn` as
`coreutils::stdio::StdioFile` already does -- copy what fits, write the full
buffer, write whole blocks of the rest directly, keep the remainder -- or move
the programs whose upstream depends on glibc's arithmetic onto `StdioFile`.
Either way re-run every harness that has a `/dev/full` case afterwards, since
which flush meets a full disk can change, and with it a message or a status.
