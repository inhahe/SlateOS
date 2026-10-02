## 1151. `tmpfile`'s file keeps its name until the stream lets go of it, and a temporary name's directory test leaves `errno` alone

**Date:** 2026-09-30
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** a file from `tmpfile` must vanish when it is closed or the
program exits. glibc makes that happen by deleting the file's name the
moment it is created -- the open file carries on without one, and the
system reclaims it at the last close. That trick does not work on this
kernel: a descriptor reaches its file by name, so deleting the name cuts the
program off from its own file. So `tmpfile` here keeps the name and deletes
it when the stream is done with the file: `fclose`, `freopen` onto another
file, or `exit`. The one thing a user could notice is that the file is
visible in `/tmp` while the program has it open -- and that a program
killed before it exits leaves the file behind, which the C standard allows.

| | glibc 2.39 | here |
|---|---|---|
| the file while open | no name (`O_TMPFILE`, or unlinked at once); `fstat` says 0 links | `/tmp/tmpfXXXXXX`; 1 link |
| removed | at the last close, by the kernel | at `fclose`, at `freopen` onto another file, at `exit` -- by the process that made it |
| a program killed, or ending by `_exit` | removed | left in `/tmp` ("implementation-defined", ISO C 7.21.4.3) |
| a child that inherited the stream closes it | nothing happens to the parent's | nothing happens to the parent's (the child does not remove it) |

**The alternatives:** unlink at once as glibc does, which here loses the
file's contents to the program itself -- every read and write after it
fails; keep the old behaviour, never removing the file, which filled `/tmp`;
or refuse `tmpfile` (`EOPNOTSUPP`) until the kernel can do it, which breaks
every program that uses it for a file that works in every other way. The
name-while-open difference is the least of these, and it is temporary:
`O_TMPFILE` is tried first, so the day the kernel supports it `tmpfile` is
glibc's with no change here; if instead descriptors come to outlive their
names, one line makes `tmpfile` unlink at once
(`known-issues.md` → `D-POSIX-TMPFILE-WAS-NEVER-REMOVED-AND-MKSTEMP-WAS-NOT-GLIBCS`).

**Second, smaller:** glibc's test of whether a directory exists (for
`tempnam`, `tmpnam` and `tmpfile`) is a `stat` whose failure it leaves in
`errno`, so a `tempnam` that succeeds after passing over a missing
`$TMPDIR` answers `errno` `ENOENT` -- undoing the care glibc's own
`__gen_tempname` takes to leave `errno` as it was on success. POSIX leaves
`errno` after a success unspecified, so neither is wrong; this library
leaves it alone in both places, as it did before. The oracle's `tempnam`
lines that say `ENOENT` after a name are the cases (`tempname.rs`'s
`tempnam_is_glibcs` states the difference).

**Where:** `posix/src/stdlib.rs` (`tmpfile`); `posix/src/stdio.rs`
(`hold_temporary`, `release_temporary`, and `fclose`, `freopen` and
`exit_cleanup` calling it); `posix/src/tempname.rs` (`dir_exists`).
