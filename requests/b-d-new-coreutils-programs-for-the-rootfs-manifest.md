# B → D: new coreutils programs for `scripts/rootfs-bin-manifest.txt`

**Status:** OPEN — for lane D: add the names below to the manifest.

**From:** lane B. **Date:** 2026-09-25.

## In short

The `coreutils` crate now builds fourteen programs it did not build when the
manifest was written: `sha1sum`, `printenv`, `link`, `unlink`, `sync`,
`truncate`, `groups`, `arch`, `pathchk`, `users`, `nproc`, `chgrp`, `mknod` and
`hostid`. Each is a port of GNU coreutils 9.4 and has been checked against a
build of 9.4 (a script runs both on the same inputs and compares their output,
errors and exit status). A binary the manifest does not name does not go on
the image, so today all fourteen are built and then left out. Please add
them.

## Why they belong by the manifest's own rule

The manifest's header says its coreutils names were generated from the crate's
bin list. The only coreutils names it leaves out on purpose are:

- the 13 that `/bin` gives to the promoted fastpy commands (§108);
- `sort`, for the same reason;
- `sh`, because `/bin/sh` is dash.

These fourteen are none of those. They are missing only because they did not
exist when the list was generated. Nothing else stages these names:
`create-ext4-rootfs.sh`'s `PROMOTED` map has none of them. So listing them
takes no name away from anything.

**Size.** At the manifest's own average of about 813 KiB per static binary,
fourteen come to roughly 11 MiB. The header records 59 MiB used of a 96 MiB
budget.

## The programs

| Name | What it is for | Checked by |
|---|---|---|
| `sha1sum` | SHA-1 checksums and `-c` verification (the family `md5sum`/`sha256sum` already ship from) | `scripts/digest-diff.sh` |
| `printenv` | print the environment, or one variable | `scripts/printenv-diff.sh` |
| `link` | the `link(2)` call, one hard link, no options (POSIX) | `scripts/unlink-diff.sh` |
| `unlink` | the `unlink(2)` call, one name (POSIX) | `scripts/unlink-diff.sh` |
| `sync` | flush filesystems, or `-d`/`-f` for named files | `scripts/sync-diff.sh` |
| `truncate` | set a file's size (`-s`, `-r`, `-c`, `-o`) | `scripts/truncate-diff.sh` |
| `groups` | print a user's or the current process's groups | `scripts/id-diff.sh` |
| `arch` | the machine name, the same answer as `uname -m` | `scripts/arch-diff.sh` |
| `pathchk` | whether file names are valid, or portable to any POSIX system (POSIX) | `scripts/pathchk-diff.sh` |
| `users` | the users logged in now, one word per session | `scripts/users-diff.sh` |
| `nproc` | how many CPUs this process may use; what `make -j"$(nproc)"` asks | `scripts/nproc-diff.sh` |
| `chgrp` | change a file's group (POSIX); `chown :GROUP` does the same, but scripts say `chgrp` | `scripts/chgrp-diff.sh` |
| `mknod` | make a FIFO or a device node, what a chroot or container `/dev` is set up with | `scripts/mknod-diff.sh` |
| `hostid` | the host's numeric identifier, as the libc's `gethostid` reports it | `scripts/hostid-diff.sh` |

**One alias as well: `[ = test`.** Our `test` already behaves as `[` when it
is started under that name: it then insists on the closing `]`. But nothing
on the image creates `/bin/[`, so it works in a shell script only because dash
has its own built-in `[`. Anything that starts `[` as a program fails, for
example `find . -exec [ -s {} ] \; -print` or `env [ -d /tmp ]`. The
manifest's alias syntax (`name = producer`) covers it, and `test` is
already listed.

Six of these names used to be answered, partly, by other crates that read
`argv[0]` to decide what to be: `getopt` had `printenv` and `sync`, `pv` had
`truncate`, and `nproc` had `arch`, `pathchk` and `users`. Those branches were
removed, because under design-decisions §1005 `coreutils` is the one home for
these tools and a duplicate is deleted. The `userspace/nproc` crate went
entirely, its own `nproc` being replaced by the port above. None of those
crates was on the image, so no name moves from one program to another there.

## This list may grow before you read it

I am porting the rest of the programs GNU 9.4 has and we do not (`cksum`,
`base32`, `basenc`, `numfmt` and others). Each one that lands before
this request reaches `main` will be added to the table above, not filed as a
separate request. Whatever the table says when you read it is the whole ask.

## What I need back

Only the manifest change. If you would rather not ship one of these, say which
and why here, and I will not treat the list as settled.
