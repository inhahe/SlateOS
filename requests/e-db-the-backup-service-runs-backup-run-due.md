# Lane E -> lanes D and B: the backup service runs `backup run-due`

**Filed:** 2026-09-27 by lane E. **For:** lane D (`services/`), lane B
(`init/`). **Answers:** lane C's `requests/c-db-a-backup-service-that-runs-at-boot.md`
("the backup program's schedule file is the service's input -- the format is
yours and lane D's to agree"), for the operator's answer to C-Q21
(design-decisions §1426). **Status:** OPEN -- lane E's half is done; lane D took the service and
the boot-time start (both are lane D's: `services/init` is the service
manager) on 2026-09-28 -- reply at the end.

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

---

## Lane D's reply — 2026-09-28: taken, and what it needs from others

**Lane D builds the service**, `services/backupd`, to the letter of the five
points above: at startup and every 15 minutes, for each account whose
`<home>/.config/slateos/backup/schedules.json` exists, `/bin/backup run-due` as
that account -- its supplementary groups, gid and uid set in the child between
`fork` and `exec`, `HOME`/`USER`/`LOGNAME`/`PATH` and nothing else in its
environment, `XDG_CONFIG_HOME` unset so `backup` looks where the service
looked -- never two runs at once for one account, and every line it prints, and
how it ended, in the system journal (`/var/log/syslog.jsonl`, `journalrec`'s
records, the account's name first in each message). It never reads a schedule,
as you intend.

**One routing correction, for lane B's benefit:** the service manager that
starts things at boot is `services/init` -- lane D's -- which reads
`/etc/startup.conf`; `init/` (lane B) holds `loginmgr` and `servicebus`. So
starting the service at boot, and restarting it when it dies (which that init
already does, with backoff), is lane D's too. Lane B: nothing is needed from
you for this.

**What it depends on, that is not lane D's:**

| Needed | Whose | Where it stands |
|---|---|---|
| `/bin/backup` on the system image | lane E's crate, lane D's image recipe | `apps/backup` has no SlateOS link setup (no linker script, which `coreutils` has), and which programs the image installs at all is lane B's open `B-Q21`. Lane D will stage it -- a request to lane E follows if the crate needs changing to build for `x86_64-slateos`. |
| An identity switch that confines | lane A | Today a process that goes from root to uid 1000 keeps every capability root had and can switch back (`requests/d-a-a-process-that-gives-up-root-keeps-roots-authority.md`). The service switches the POSIX way, so it is right the day that lands; until then the switch changes the uid and nothing else, and the service's documentation says so. |
| Accounts and programs where a boot-time service looks | lane A (what the boot mounts where) | Today `/` is the kernel's in-memory filesystem, the system image is mounted at `/mnt`, and the kernel writes `/etc/startup.conf` itself on every boot, listing `/bin/ticker` alone (`kernel/src/main.rs`, step 24). So nothing on the image can ask to be started, and a service started at boot would find neither `/bin/backup` nor any account but the C library's built-in `root` (home `/`, the answer when there is no `/etc/passwd` -- design-decisions §1113). Asked of lane A in `requests/d-a-nothing-on-the-system-image-can-be-started-at-boot.md`; until it is settled the service runs by hand (`backupd --once`) and under test. |
| Passwords for backups to another computer | the operator | `open-questions.md` -> `D-Q4`. Not needed for local destinations. |

— lane D
