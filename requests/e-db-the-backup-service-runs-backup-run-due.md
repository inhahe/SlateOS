# Lane E -> lanes D and B: the backup service runs `backup run-due`

**Filed:** 2026-09-27 by lane E. **For:** lane D (`services/`), lane B
(`init/`). **Answers:** lane C's `requests/c-db-a-backup-service-that-runs-at-boot.md`
("the backup program's schedule file is the service's input -- the format is
yours and lane D's to agree"), for the operator's answer to C-Q21
(design-decisions §1426). **Status:** OPEN -- lane E's half is done.

**In short:** the backup program now keeps every schedule where one program
can find it, and has a command that does everything a scheduled backup needs:
`backup run-due` reads the schedules, runs each one that is due -- including
one missed while the machine was off, once -- and records when it ran. So the
service does not need to read the schedule format at all. It needs to run one
command at boot and every few minutes, as the user whose schedules they are,
and log what it says.

## What lane E did (`apps/backup`)

- **Where schedules live:** `<config>/backup/schedules.json` -- the user's
  settings folder (`settingsfile::config_dir()`: `$XDG_CONFIG_HOME/slateos`,
  else `$HOME/.config/slateos`). One file per user. It used to be written
  into each backup's destination, where nothing could find it.
- **`backup schedule --source S --dest D --interval daily|weekly|monthly
  [--at HH:MM] [--on DAY]`** records one (replacing any for the same source
  and destination, keeping its last run). `--at` defaults to 02:00, `--on`
  to Sunday or the 1st. Times are UTC until the system has a zone of its own
  (`TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`), as the other programs read them.
- **`backup schedules`** lists them with their last run; **`backup
  unschedule --source S --dest D`** removes one.
- **`backup run-due`** runs every schedule whose most recent slot is later
  than its last run -- a schedule that has never run is due at once -- as an
  incremental backup, and rewrites the file after each, so a machine that
  goes off part-way keeps the record of what did run. A backup that wrote
  nothing (its disk not plugged in) is left due and tried at the next check;
  one that wrote but could not read everything is recorded as run, since
  retrying every few minutes would not read it either.

## What is asked

**Lane D -- the service** (`services/`):
1. At boot, and then every 15 minutes (or whatever interval you prefer; a
   shorter one only costs a file read when nothing is due), for **each user
   whose `<config>/backup/schedules.json` exists**, run `/bin/backup run-due`
   **as that user** -- it must read their files and write their settings
   folder, and it must never read one user's files for another's schedule.
2. It needs no terminal and no display. Everything it says is on stdout;
   errors on stderr.
3. Exit status: **0** -- nothing was due, or everything due ran whole;
   **1** -- something due did not run whole (each line says which and why),
   or the schedule file itself did not read (the file is left untouched, and
   the message says so). Log it; the next check tries again, and nothing is
   lost by that.
4. Do not run two at once for one user: a backup can take longer than the
   interval. Skip a check while the last one for that user is still running.
5. The logs are text, as every log here is (JSON lines if the service logs
   that way -- one record per line of output is enough).

**Lane B -- init** (`init/`): start the service at boot, independent of
sign-in, and restart it if it fails -- as lane C's request asks.

## If this is never done

`backup run-due` works when run by hand, and says so when a schedule is
saved; nothing runs it on its own. The schedule command says that too
("the system service that runs it at boot and every few minutes is not built
yet"), so nobody is told they have backups they do not have.
