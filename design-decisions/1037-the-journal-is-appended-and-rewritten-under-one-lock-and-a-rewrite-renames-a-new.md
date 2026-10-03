## 1037. The journal is appended and rewritten under one lock, and a rewrite renames a new file over the old

**Date:** 2026-09-26
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** three programs add records to the system log file,
`/var/log/syslog.jsonl` -- `syslogd`, `logger` and `systemd-cat` -- and
three things shrink or move it: `journalctl --vacuum-time`/`--vacuum-size`,
`syslogd clean`, and `syslogd`'s own rotation when the file passes 5 MiB. A
vacuum used to read the file, drop the old records, and write the rest back
over the same file; a record another program added between that read and
that write was simply gone. Now every one of them takes a lock on the file
first. A program adding a record waits while a vacuum or rotation holds it,
then adds its record to whichever file is there afterwards; a vacuum writes
its result as a new file and renames it into place while it still holds the
lock. No record is lost, and a vacuum that dies half way leaves the old file
whole.

| Option | *What changes:* | For | Against |
|---|---|---|---|
| **Advisory `flock` on the file; rewrites rename a new file over it (chosen)** | nothing a user sees, except that records are no longer lost | every writer is ours and already opens, writes one record and closes, so taking a lock is three lines in each; readers are untouched; the rename makes the rewrite crash-safe; it also closes the same race in `syslogd`'s rotation, which moved a waiting writer's record into `.1` | only programs following the protocol are held to it; needs `flock`, which SlateOS's C library has |
| Rotate instead of rewriting (the entry's first sketch): rename the live file aside, filter it at leisure | the vacuum leaves renamed files behind | no locking | a writer that opened the file just before the rename still writes to the renamed one after it was read -- the same loss, in a smaller window; every reader must learn the new files |
| Route every write through `syslogd` | one writer, so no race | the textbook shape | SlateOS has no Unix-domain sockets yet (`logger` writes the file itself for that reason), so the daemon cannot be reached |

How it works (`userspace/journalio`): a writer opens the file for
appending, takes `flock(LOCK_EX)`, and -- holding it -- checks that the path
still names the file it opened (device and inode), since a rewrite or a
rotation may have replaced it while it waited; if not, it opens the path
again. A rewriter takes the lock with the same check, reads, writes the new
contents to `.NAME.PID.tmp` beside the log with the old file's mode and
owner, syncs it, and renames it over the log before releasing the lock. A
rotation renames the locked file away. `flock` locks belong to an open file,
so two threads of one program exclude each other as two programs do. Where a
file system has no locks (`ENOLCK`, `ENOSYS`, `EOPNOTSUPP`), a writer appends
without one, as before -- a record written beats one refused -- and a rewrite
refuses, since rewriting unlocked is the loss this exists to prevent.

`syslogd`'s rotation takes the lock for every step, the older copies' shifts
included: a vacuum rewriting `syslog.jsonl.1` would otherwise rename its
result over whatever the rotation had just moved there. And `journalctl` now
reads the rotated copies (`syslog.jsonl.N`, oldest first), whose records it
had never shown, and vacuums them too, removing one it empties.

**Where:** `userspace/journalio` (the protocol, with tests that fail when the
lock is taken out); `userspace/syslogd` (entries, `clean`, rotation),
`userspace/logger` (`deliver::append_record`), `userspace/systemctl`
(`systemd-cat`, one append per record), `userspace/journalctl` (both vacuums,
and the rotated copies). Known-issues
B-JOURNALCTL-VACUUM-LOSES-RECORDS-APPENDED-DURING-ITS-REWRITE.

**Revisit** when SlateOS has Unix-domain sockets and `syslogd` receives on
`/dev/log`: with one writer, the lock would only be between it and the
rewriters.
