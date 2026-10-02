## TD-B-PS-IS-NOT-PROCPS-AND-NAMES-EVERY-TERMINAL-A-PTY (lane B, 2026-10-02)

**Status:** OPEN -- the proper fix is a port of procps-ng 4.0.4's `ps`, which
is not started.

**In short:** `ps` in coreutils (`userspace/coreutils/src/bin/ps.rs`) was
written here, not ported -- `ps [-e] [-f]` and a few selections, where procps'
`ps` has three option syntaxes and some ninety output fields -- and its TTY
column cannot name a terminal correctly. It prints every non-zero `tty_nr` as
`pts/<low byte>`: the console (`5:1`) would show as `pts/1`, and `/dev/tty2`
(`4:2`) as `pts/2`. procps decodes the number into a major and minor and asks
`/dev` which device has them (`library/devname.c`).

**Why it has not mattered yet, and soon will:** SlateOS's kernel writes 0
for every process's `tty_nr`, so today every row prints `?`, which is wrong in
a way that does not look wrong. `requests/b-ad-proc-stat-reports-no-controlling-terminal.md`
asks lanes A and D for real terminal numbers; the day it lands, this column
starts printing plausible names for the wrong terminals.

**The proper fix:** port procps-ng 4.0.4's `ps` -- `src/ps/` and the parts of
`library/` it reads (`readproc`, `devname`, `escape`) -- into coreutils with a
harness against the pristine build, as `w` was (`scripts/w-diff.sh`,
`scripts/procps-ref.sh`, which builds the reference and can build `ps` the
same way). `coreutils::procps` already holds `readproc`'s rules for the items
`w` reads; `ps` reads more of them.

**Smaller fix, if the port waits:** decode `tty_nr` as procps' `dev_to_tty`
does -- `major = (tty_nr >> 8) & 0xfff`, `minor = (tty_nr & 0xff) | ((tty_nr
>> 12) & 0xfff00)` -- and name `136:N` `pts/N`, `4:N` `ttyN`, `5:1`
`console`, anything else `?`. That is a stopgap toward the port, not instead
of it.
