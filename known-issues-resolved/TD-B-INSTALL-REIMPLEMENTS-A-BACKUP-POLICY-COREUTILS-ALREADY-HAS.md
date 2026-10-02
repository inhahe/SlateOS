## TD-B-INSTALL-REIMPLEMENTS-A-BACKUP-POLICY-COREUTILS-ALREADY-HAS (lane B, 2026-09-11) — **FIXED** 2026-10-02

**Fixed** by the route this entry pointed at: `install` is a coreutils bin
again, `userspace/coreutils/src/bin/install.rs`, a port of GNU coreutils
9.4's `install.c` on the copy engine `cp` and `mv` share (with GNU's
`set_mode`, which only `install` sets, added to it) and on a new
`coreutils::mkdirp` (gnulib's `mkdir-p.c`, `mkancesdirs.c` and
`dirchownmod.c`). Backups are `backup.rs`'s, so numbered backups,
`--backup=CONTROL`, `$VERSION_CONTROL` and `$SIMPLE_BACKUP_SUFFIX` all work,
and argv is bytes. `userspace/install` is deleted (§1005).

Measured, not assumed: `scripts/install-diff.sh` (new; the files left behind
are compared, not just the output) found the standalone agreeing with GNU on
22 of 173 cases, and finds the port agreeing on all 189 it has now --
modes and umask, `-d`, `-D`, `-t`, `-T`, every backup word and both
variables, `-C`, `-p`, `-o`/`-g`, `-s` with a strip program that records its
arguments, `-v`, the SELinux options on a kernel without it, one file under
two names, and names with a newline or a non-UTF-8 byte. `cp-diff.sh`
(584/0/30) and `mv-diff.sh` (363/0/10) did not move across the engine change.

What follows is the entry as it was filed.

**In short:** `userspace/install` has its own backup handling, and
`userspace/coreutils/src/backup.rs` has a complete, correct one that `cp`, `mv`
and `ln` already share. The separate copy is missing most of what GNU's
`install` actually does, and got the one thing it does implement wrong until
today.

**What coreutils' has and install's does not:**

| | coreutils `backup.rs` | `userspace/install` |
|---|---|---|
| simple backups (`file~`) | yes | yes |
| numbered backups (`file.~1~`) | yes | **no** |
| `--backup=CONTROL` | yes | **no** — only the bare `-b` |
| `$VERSION_CONTROL` | yes | **no** |
| `$SIMPLE_BACKUP_SUFFIX` | yes | **no** — only `-S` |
| suffix type | `Vec<u8>` | `String` |

So `install --backup=numbered` and `VERSION_CONTROL=numbered install …` both
silently make a simple backup, overwriting the previous one. GNU makes a new
`.~N~` each time; ours destroys the older copy, which is the opposite of what
someone asking for numbered backups wants.

**Why the suffix type matters too.** `install` reads `-S` into a `String`,
which means `env::args()`, which panics on a non-UTF-8 argument. coreutils
takes it as `&OsStr` and keeps it as bytes. A suffix is a filename fragment and
this filesystem allows any byte but `/` and NUL.

**The fix is not to port the missing features into install.** GNU's `install`
*is* part of coreutils; ours is a separate crate that duplicates a policy
module sitting a directory away. Either it becomes a `coreutils/src/bin`
personality — which is where the roadmap's own §1005 "coreutils is the one
home" ruling points — or `backup.rs` moves somewhere both can reach, as
`quoting::with_suffix` just did for the one byte-level primitive all of them
needed.

**How it was found:** repairing the lossy `format!("{}{}", dst.display(), …)`
in install's backup path, then looking for the same shape elsewhere and finding
that the correct byte-wise implementation had existed in `backup.rs` the whole
time.
