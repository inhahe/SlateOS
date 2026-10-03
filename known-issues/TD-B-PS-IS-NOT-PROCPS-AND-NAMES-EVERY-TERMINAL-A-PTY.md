## TD-B-PS-IS-NOT-PROCPS-AND-NAMES-EVERY-TERMINAL-A-PTY (lane B, 2026-10-02)

**Status:** FIXED 2026-10-02 (lane B), boot confirmation pending. `ps` is
procps-ng 4.0.4's, ported into coreutils (`userspace/coreutils/src/bin/ps/`),
with the library parts it reads -- `readproc`, `devname`, `pwcache`, and the
`pid_max`/`btime`/`MemTotal` readers -- in `coreutils::procps`, which `w` now
reads `/proc` through too. `scripts/ps-diff.sh` measures it against the
release built as SlateOS is (no logind, no libnuma): 586 cases agree, and 4
differ on purpose (`--version`, `-V`, `V`, `--info`). The `ps` it replaces,
measured by the same harness, agreed on 26.

The TTY column is now `dev_to_tty`'s: a terminal is named only when a file in
`/dev` has its device number, so on SlateOS it stays `?` until
`requests/b-ad-proc-stat-reports-no-controlling-terminal.md` is done -- and
then names the right terminal, not a plausible one.

**How the harness pins a moving table:** each case runs both programs in a
new user, mount and PID namespace with a fixture tree bind-mounted over
`/proc` (the program is PID 1 and the fixture's `1/` describes it), the
password files bind-mounted to fixtures, and a fresh devpts instance holding
`pts/0`-`pts/2`. Six fixture worlds -- a desktop booted in 2023, one booted two
hours ago, one without `task/` directories, one of malformed files, and some
with a system file missing -- cover every column, selection, sort, format,
personality and error path upstream has, 590 cases in all.

**What was found on the way,** all kept because upstream does them: `status`
overrides `stat` (a `status` without `Pid:` makes the PID 0); a short signal
mask takes the next line with it; `--signames` reads the hexadecimal masks as
*decimal*; BSD `O`'s second sort key is never sorted on; `--sort=utime` sorts
nothing (its key is never registered); `-D%H` reads its own format again as
options; and a process with no `status` shows `[ duplicate SUPGIDS ]`. Two
upstream behaviours are not kept, as `main.rs` lists: `-m` looping forever
when a thread group's first entry is not its leader, and the undefined
behaviour (buffer overruns, a division by zero, an uninitialised `double`)
behind a few malformed inputs.

---

*The entry as filed:*

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
