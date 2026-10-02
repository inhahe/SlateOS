## TD-B-TABLE-PROGRAMS-LAY-OUT-THEIR-OWN-TABLES (lane B, 2026-09-26) — **open**

**In short:** the util-linux programs that print tables do not decide their
own column widths upstream -- a library, libsmartcols, does -- but ours each
lay out their tables themselves. So on a narrow terminal each cuts
differently from util-linux (and from one another), and `--json`, `--raw`
and `--pairs` are each program's own dialect. The library is ported now, as
`userspace/smartcols` (design-decisions §1036), and `lsmem` prints through
it and matches util-linux 2.39.3 at every terminal width
(`scripts/lsmem-diff.sh`), and so does `prlimit` (`scripts/prlimit-diff.sh`:
182 cases, plus five narrow widths at which only upstream never finishes),
and so does `column` (`scripts/column-diff.sh`: 594 cases in all its
modes, plus 9 at which only upstream never finishes -- a port that
needed the column moves, re-parenting and `--table-column` properties added
to the crate), and so does `lsirq` (`scripts/lsirq-diff.sh`: 212 cases on
`/proc` files of its own, plus 1 at which only upstream never finishes),
and so does `lscpu` (`scripts/lscpu-diff.sh`: 1421 cases on
util-linux's snapshots of nineteen machines, trees of its own and WSL
itself, plus 3 narrow terminals at which only upstream never
finishes -- a port that needed tree symbols of the program's own added to
the crate), and so does `findmnt` (`scripts/findmnt-diff.sh`: 4953 cases on
util-linux's own test tables, tables of its own and WSL's, including
`--verify` and `--poll`, plus 8 narrow terminals at which only upstream
never finishes -- a port that needed libmount's table code and libblkid's
device cache, as `userspace/ulmount`, and newline-wrapped cells, JSON arrays
and range printing added to the crate; `mountpoint`, which the old program
doubled as, is its own port now, `scripts/mountpoint-diff.sh`), and so does
`lsns` (`scripts/lsns-diff.sh`: 483 cases, run inside a user, PID, network
and mount namespace the harness makes for itself -- every type of namespace,
user and PID namespaces nested three deep, persistent ones, assigned network
IDs -- plus 4 where only upstream crashes, dereferencing the missing process
of a persistent namespace whose owner `-t` filtered out), and so does `lsblk`
(`scripts/lsblk-diff.sh`: 502 cases on util-linux's `--sysroot` snapshots
and WSL itself -- a port that needed libsmartcols' sorting and line groups,
measured on their own against the real library by
`scripts/smartcols-diff.sh`: 2954 tables), and so does `swapon`'s `--show`
(`scripts/swapon-diff.sh`, 166 cases, 2026-09-27); the others still do not use
it.

**Where:** `losetup` (`--list`), `rfkill`, `fdisk` (`-l`'s partition
table). `rfkill`'s reference, like `lsirq`'s,
is not installed in WSL; `scripts/util-linux-extra.sh` unpacks both without
root.

**The proper fix:** port each program from util-linux 2.39.3 onto
`smartcols`, as `lsmem` was -- the program's own logic function by function,
the table handed to the crate -- with a differential harness against WSL's
util-linux that includes a pty at several widths. Parts of libsmartcols not
yet ported (the crate's module docs list them: custom wrap functions other
than the newline one, and colours) are added when a program needs them;
groups and sorting are in since lsblk's port. Each of these is
also on TD-B-STANDALONE-PORTS-MATCH-LONG-OPTIONS-WHOLE's list, and the two
are one job per program.
