## B-JOURNALCTL-VACUUM-LOSES-RECORDS-APPENDED-DURING-ITS-REWRITE (lane B, 2026-09-26) — ✅ FIXED 2026-09-26 (lane B)

**In short:** `--vacuum-time` and `--vacuum-size` read a log file, filter it,
and write the survivors back over the same path. A record another program
appends between the read and the write is overwritten and lost. Every writer
here appends one record at a time (`syslogd log`, `systemd-cat`, `logger`),
so a vacuum run on a live system can silently drop whatever was logged while
it ran.

**Where:** `cmd_vacuum_time` and `cmd_vacuum_size` in
`userspace/journalctl/src/main.rs` (`fs::read`, then `fs::write` of the kept
lines). Found while fixing their reads; not reproduced.

**The proper fix** is to stop rewriting a live file: rotate instead -- rename
the live file aside (writers that open, append and close per record then
start a fresh one), filter the renamed file at leisure, and have the readers
include rotated files. That changes where `journalctl` finds records outside
`/var/log/journal/`, so it is a design change of its own, not a patch to the
vacuum.

**Fixed** with a lock rather than rotation (design-decisions §1037, which
weighs the two: a writer that opened the file just before a rotation's rename
would still have written into the renamed file after it was read). Every
writer -- `syslogd`, `logger`, `systemd-cat` -- appends through
`journalio::append`: `flock`, check the path still names the file, write,
close. Every rewriter -- both vacuums and `syslogd clean` -- holds the lock
from its read to a rename of a new file over the old one, and `syslogd`'s
rotation holds it for each rename. A writer arriving meanwhile waits, then
lands in the new file; `journalio`'s two race tests fail when the lock is
removed. Two more defects went with it: `syslogd clean` refused a whole log
for one byte that was not UTF-8, and rewrote the file when it had removed
nothing; and `journalctl` never read `syslogd`'s rotated copies
(`syslog.jsonl.1` ...), so every record older than the last rotation was
invisible -- it reads them now, oldest first, and vacuums them.
