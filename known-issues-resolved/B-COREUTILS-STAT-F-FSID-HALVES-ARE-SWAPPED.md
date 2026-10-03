## B-COREUTILS-STAT-F-FSID-HALVES-ARE-SWAPPED (lane B, 2026-09-11)

**Status:** FIXED 2026-09-14 (lane B, `2a4d423cf`), found closed 2026-10-03.
`stat` calls `statfs`, as GNU's does on Linux, and builds `%i` as GNU does
from the two words -- `val[0]` high, `val[1]` low -- so the halves are no
longer swapped; and `statfs`'s `f_type` gives `%t` and `%T` the magic and
the name too, so the paragraph below calling them "not defects" no longer
describes the program. `scripts/stat-diff.sh` runs `-f -c %i`, `%t` and
`%T` as ordinary cases. The commit has been on `main`, and through its boot
tests, since 2026-09-14; the entry was simply never closed.

*The entry as filed:*

`stat -f -c %i` prints the filesystem id with its two 32-bit halves exchanged
relative to GNU:

    GNU    68867c45465b201c
    ours   465b201c68867c45

The cause is that the two programs read different syscalls. GNU uses Linux
`statfs`, whose `f_fsid` is `struct { int val[2]; }` and which it renders half
by half; ours uses POSIX `statvfs`, whose `f_fsid` is a single `unsigned long`
that glibc packs in the opposite order.

**Deliberately not "fixed" here.** Swapping the halves would match glibc on
Linux and could be *wrong on the target* — SlateOS's own `statvfs` is free to
pack its id however it likes, and compensating for a glibc detail in the
renderer would bake a host assumption into a program that does not run on the
host. Whoever fixes this should first measure what SlateOS's `statvfs` returns;
the fix is then one line in the `b'i'` arm.

### Not defects, recorded so they are not re-filed

**`%t` and `%T`** print `?` and `UNKNOWN` where GNU prints `ef53` and
`ext2/ext3`. `stat.rs` documents this at the call site: *"`statvfs` carries no
filesystem-type field, so there is nothing to print. GNU prints `?` here too on
a kernel whose `statfs` lacks `f_type`; this is that same case, not a new
one."* A consequence of the syscall choice, already known.

**`%a` and `%f`** were listed here as disagreeing and **do not**. They were an
artefact of the harness — see below — and now pass.
