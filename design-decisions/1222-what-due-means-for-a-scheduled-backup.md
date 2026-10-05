## 1222. What "due" means for a scheduled backup

**Date:** 2026-09-27
**Lane:** E
**Decided by:** Claude (autonomous), inside the operator's answer to C-Q21
(lane C's §1426: a service runs backups at boot, and a missed one runs as
soon as the machine is on again, without asking)

**In short:** `backup run-due` runs a schedule when the most recent moment it
names -- today at 02:00, last Sunday at 02:00, the 1st of this month -- is
later than its last run. That one comparison gives the operator's rule for
free: a backup missed while the machine was off is due at the next check, and
however many were missed, it runs once. Four smaller calls sit inside it.

| Call | Chosen | The other way, and why not |
|---|---|---|
| A schedule that has never run | due at once | wait for its first time: someone who has just asked for nightly backups has none until tonight |
| A last run *after* now | due | wait for the clock to catch up: only a clock that was wrong and was put right makes one, and waiting could mean years with no backup |
| A day the month lacks (the 31st in February) | its last day | skip the month: a monthly backup that silently skips months is not monthly |
| A destination with no store (a disk not plugged in) | not written, tried again at the next check | make the store: `Store::open` would build it in the empty mount point on the system disk, and the owner would believe the backup was on the other |

And when a run is recorded: a backup that was written counts as run even if
some files could not be read -- retaking it every few minutes would not read
them either, and the run says so and fails its exit status -- while one that
wrote nothing is left due.

**The time zone.** A schedule's time is read in the desktop's zone, which is
UTC until the system has one (`TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`), as
`notes`, `habits` and `finance` read theirs; `zone_offset` in `apps/backup` is
the one place that changes when it does.
