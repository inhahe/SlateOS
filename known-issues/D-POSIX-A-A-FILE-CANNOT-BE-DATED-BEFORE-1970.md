## D-POSIX-A-A-FILE-CANNOT-BE-DATED-BEFORE-1970 — a file's times cannot be set to the epoch or any time before it (lane D, 2026-10-16)
**Status:** OPEN -- needs a signed native time from lane A; nothing on lane D's side can fix it

**In short:** Linux lets a program date a file before 1970 -- `touch -d
1969-07-20 f`, or `tar` restoring an old archive -- and `stat` then shows that
date. Here the file is dated 1970-01-01 00:00:00.000000001 instead, and a
time of exactly the epoch (`touch -d @0 f`) gets the same nanosecond. Nothing
reports it: the call succeeds, as Linux's does when a filesystem cannot hold a
time and it clamps the time into the range it can.

**Where:** the native set-times calls (`SYS_FS_SET_TIMES` 632,
`SYS_FS_UTIMENSAT_PINNED` 669) take each time as a `u64` count of nanoseconds
since the epoch, and keep both of its ends as requests: 0 is "leave it as it
is" and `u64::MAX` is "now" (`kernel/src/fs/vfs.rs`, `TIME_NOW`). So the
earliest time one can carry is 1 ns, and `posix/src/file.rs`, `given_time_ns`,
clamps every given time into `1 ..= u64::MAX - 1`. The kernel's own
`Timestamp` is the same unsigned count, so a file could not hold an earlier
time even if the call could carry it.

The Linux-ABI `utimensat` (`kernel/src/syscall/linux.rs`, `sys_utimensat`'s
`conv`) differs: it turns a time `<= 0` into "leave it as it is", so from a
Linux binary the same `touch` succeeds and changes nothing. Pointed out to
lane A in `requests/a-d-utime-now-is-a-request.md`.

**Reproduce:** on SlateOS, `utimensat(AT_FDCWD, "f", {{-86400, 0}, {-86400,
0}}, 0)` then `stat("f")`: `st_mtim` is `{0, 1}`, where Linux on ext4 answers
`{-86400, 0}`.

**The proper fix:** a signed native time -- `i64` nanoseconds, which reach
from 1677 to 2262 -- in the kernel's `Timestamp` and in the set-times calls,
with the two requests moved out of the time's range into a flag word, as
Linux's `UTIME_NOW`/`UTIME_OMIT` sit in `tv_nsec` rather than in the time.
ext4 stores times before 1970 already (its inode times are signed seconds
with an extra epoch field), so the filesystem is not the obstacle. The C
library then passes the time through unclamped.
