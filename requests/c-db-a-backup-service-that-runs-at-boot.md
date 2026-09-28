# C -> D, B -- a backup service that runs at boot (the operator's answer to C-Q21)

**From:** Lane C. **To:** Lane D (`services/**`) and lane B (`init/**`).
**Filed:** 2026-09-27. **Status:** OPEN -- decided by the operator; the work is
yours.

**In short:** setting a backup to run every day does nothing today: the backup
program writes the schedule to a file nothing reads
(`known-issues.md` `BUG-C-BACKUP-SCHEDULE-WRITES-A-FILE-NOTHING-EVER-READS`).
The operator chose what runs it (`design-decisions.md` §1426): a background
service the system starts at boot, so a daily backup happens on time whether or
not anyone is signed in. **And a backup missed because the machine was off runs
as soon as the machine is on again -- before anyone signs in, and without
asking.**

The operator's words: "if a backup is missed because the machine is off, it
shouldn't just ask when the user signs on, it should backup if the machine comes
back on after it was missed even if the user hasn't signed on yet. And in that
case, I don't think it should ask the user whether to backup or not regardless."
And: "I guess backup should be a service handled by our startup manager ... I
don't know if that handles services run all the time too, or only things loaded
when the user logs in. I think the former?" -- which lane C answered in §1426 as
yes, the init system starts services at boot; correct that there if it is wrong.

## What is asked

- **Lane D:** a backup service in `services/` that reads the schedule the backup
  program writes, runs each backup at its time, remembers when each last ran,
  and on starting runs any that were missed while the machine was off.
- **Lane B:** init starts it at boot, independent of sign-in, and restarts it if
  it fails.
- **Lane E** (told separately): the backup program's schedule file is the
  service's input -- the format is yours and lane D's to agree.

## If this is never done

The schedule setting keeps recording a wish nothing carries out. The program
already says so, so nobody is misled; nobody gets a scheduled backup either.
The operator: "It doesn't matter how long it stays broken as long as it's fixed
by the time the OS is finished."
