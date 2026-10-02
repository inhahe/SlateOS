## D-POSIX-TMPFILE-WAS-NEVER-REMOVED-AND-MKSTEMP-WAS-NOT-GLIBCS — `tmpfile` never removed its file; `mkstemp` and its family drew six letters from 36 with a bias and gave up after 100; `mkostemp` or'd the caller's access mode into `O_RDWR`; `mktemp` never checked its name was free (lane D, 2026-09-30) — **Status: FIXED 2026-09-30 (`posix/src/tempname.rs`), but for one difference the kernel forces: see "Still open"**

**In short:** these are the functions a program calls for a scratch file or
a name for one. `tmpfile` must make a file that disappears when it is
closed or the program exits; ours left every one in `/tmp` for good, so a
program calling it in a loop filled the disk. `mkstemp` and its relatives
must pick an unused name from the template's `XXXXXX`; ours picked from
fewer letters than glibc (36, not 62), slightly unevenly, and gave up after
100 tries where glibc tries 238,328; and a failed draw of random bytes
(ignored) made every try the same name. `mkostemp(t, O_WRONLY)` asked
`open` for an access mode that does not exist. `mktemp` returned a name
without checking that nothing had it, and NULL where glibc and SUSv2
return the template emptied.

| | Was | Is (glibc 2.39's) |
|---|---|---|
| `tmpfile` | `/tmp/tmpXXXXXX`, never removed | `O_TMPFILE` if the kernel has it (it does not yet), else `/tmp/tmpfXXXXXX`, removed when the stream is closed, `freopen`ed onto another file, or at `exit` -- by the process that made it |
| a name's six bytes | `[0-9a-z]`, `byte % 36` (the first four digits a little likelier), from `getrandom`, its failure ignored | `[a-zA-Z0-9]` in glibc's order, drawn from `arc4random` without bias |
| names tried | 100, then `EEXIST` | 62 cubed (glibc's `ATTEMPTS_MIN`), then `EEXIST` |
| `mkostemp`'s flags | or'd with `O_RDWR | O_CREAT | O_EXCL` | their access mode replaced by `O_RDWR`, as glibc's `try_file` |
| `mktemp` | a name, not checked; NULL for a bad template | a name `lstat` says is free; the template always, emptied on any failure |
| `errno` after a success | whatever `open` left | as it was |
| `mkstemp64`, `mkostemp64`, `mkstemps64`, `mkostemps64`, `tmpfile64` | missing | glibc's large-file names for the same functions |

`tmpnam`, `tmpnam_r` and `tempnam` were glibc's already (the stdio rewrite,
`D-POSIX-STDIO-WAS-SIXTEEN-UNLOCKED-SLOTS`); their name generator and directory search moved into
`posix/src/tempname.rs` with the rest, so that there is one of each.

**Still open -- `tmpfile`'s file has a name while it is open.** glibc
unlinks it at once and the file lives on through the descriptor. This
kernel's descriptors reach a file through its name (`kernel/src/fs/handle.rs`
re-resolves the path on every read and write), so an unlinked open file is
lost to its own descriptor; and `O_TMPFILE` is refused
(`posix/src/file.rs`, `EOPNOTSUPP`). So the name stays until the stream lets
go of the file. What differs from glibc: another process can see
`/tmp/tmpfXXXXXX` while it is open; `fstat` says one link, not none; and a
program that ends without `exit` (`_exit`, a fault) leaves the file, which
ISO C allows ("whether an open temporary file is removed is
implementation-defined"). A stream a child inherits across `fork` is not
removed by the child. **The proper fix** is the kernel's: a descriptor that
outlives its file's name (an inode or object reference, orphan inodes
reclaimed at last close), or a working `O_TMPFILE` -- lane A's, recorded
in `todo.txt` under the `O_TMPFILE` entry. When either lands, `tmpfile`
needs no change for `O_TMPFILE` (it is tried first), or one line to unlink
at once. design-decisions.md §1151.

**Tests:** `posix/tools/oracle/tempfile_harness.py` records glibc 2.39's
answers (`tempfile_oracle.txt`, 554 probes): 34 template shapes through the
ten functions, each run eight times so that the bytes a name replaces are
told from those it keeps; the flags the `o` forms take and what the
descriptor has; the modes under three umasks; `tmpfile` with and without
`$TMPDIR`; `tempnam` over nine directories, six prefixes and four
`$TMPDIR`s; `tmpnam` and `tmpnam_r`. The tests replay them through the
functions themselves over a filesystem in memory (`tempname::fake`), and
check that `exit` and `freopen` remove what they should, once.

**Where:** `posix/src/tempname.rs` (new); `posix/src/stdlib.rs`
(`mkstemp` ... `mkdtemp`, `mktemp`, `tmpfile`); `posix/src/stdio.rs`
(`tmpnam`, `tempnam`, the stream's hold on a temporary name).
