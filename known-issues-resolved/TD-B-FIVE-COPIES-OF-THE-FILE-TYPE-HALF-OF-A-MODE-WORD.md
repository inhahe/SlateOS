## TD-B-FIVE-COPIES-OF-THE-FILE-TYPE-HALF-OF-A-MODE-WORD (lane B, 2026-08-22) — RESOLVED 2026-08-22

**In short:** A file's *mode* is one integer holding two unrelated things: what
kind of file it is (regular, directory, symlink, pipe, socket, device) and who
may read, write and run it. Five utilities each wrote out the seven "what kind"
values by hand. Two of them got it wrong in a way a user could see. They now
share one definition, in `userspace/modechange` — the crate that already owned
the other half of the same integer.

**The two real bugs, not just the duplication:**

1. **`userspace/stat` printed `unknown` where GNU prints `weird file`.** `%F` is
   the format `stat -c %F` uses to name a file's type, and a type this system
   has no name for is `weird file` in gnulib's wording (`lib/c-file-type.c`).
   A script matching on the GNU wording matched nothing.
2. **`mkinitramfs` tested the type as if it were a bit field.** Its test filter
   was `e.mode & 0o040000 != 0`, which is wrong because the `S_IF*` values are
   *not* flags: `S_IFCHR | S_IFDIR == S_IFBLK`, exactly. That filter counts a
   block device (`0o060000`) and a socket (`0o140000`) as directories. It only
   escaped notice because the fixture it ran on has neither. Now
   `mode & S_IFMT == S_IFDIR`.

**Where it lived, and what it is now:**

| Site | Before | After |
|---|---|---|
| `userspace/coreutils/src/bin/stat.rs` | own `S_IFMT` + two 7-arm matches | `modechange::{S_IFMT, S_IFREG, file_type_name, mode_string}` |
| `userspace/stat/src/main.rs` | own 8 `u64` constants + two 7-arm matches + a hand-written `strmode` | `modechange`, widened to `u64` at the boundary |
| `userspace/cpio/src/main.rs` | own 4 constants | `modechange::{S_IFDIR, S_IFLNK, S_IFMT, S_IFREG}` |
| `userspace/mkinitramfs/src/main.rs` | 5 bare octal literals | `modechange::{S_IFCHR, S_IFDIR, S_IFLNK, S_IFREG}` |
| `userspace/coreutils/src/bin/ls.rs` | (being written) | `modechange::{S_IFDIR, S_IFMT}` |

**What `modechange` gained:** `S_IFMT` and the seven type values, plus
`file_type_letter` (gnulib's `ftypelet` — the first character of `ls -l`'s mode
column), `file_type_name` (GNU `stat`'s `%F` wording) and `mode_string` (the
whole ten-character `-rw-r--r--`, which is `file_type_letter` followed by the
existing `permission_string`). Every value measured against GNU coreutils 9.4:

```text
$ stat -c '%A %F' reg d sym pipe sock /dev/null /dev/loop0
-rw-r--r-- regular file        drwxr-xr-x directory
lrwxrwxrwx symbolic link       prw-r--r-- fifo
srwxr-xr-x socket              crw-rw-rw- character special file
brw-rw---- block special file
```

`userspace/stat` and `userspace/coreutils`' `stat` remain two separate binaries
with the same name; that duplication is a different item and is blocked on
question B-Q7. This entry is only about the seven numbers they both needed.
