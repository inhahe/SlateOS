### FIXED-A-BACKUP-FREQUENCY-WAS-RECORDED-DISPLAYED-AND-NEVER-OBEYED (lane A)

**In short:** Backup schedules let you pick how often a backup should run —
hourly, daily, weekly, monthly. That setting was saved, and shown back to
you in the schedule list, but nothing anywhere in the system ever compared
it against a clock. Every schedule therefore behaved as "manual": it ran
only when someone explicitly asked, no matter what frequency was chosen.
A second bug sat underneath it — the "when did this last run?" timestamp
used zero to mean "never", but zero is also a real time (the instant the
machine booted), so a backup that had never run looked like one that had
just run. Fixed both.

**Status:** FIXED 2026-08-23, commit `dc80769bf`.

**Where:** `kernel/src/fs/backupsched.rs` — `BackupFrequency`,
`BackupSchedule::last_run_ns`, new `due_schedules()`; display in
`kernel/src/kshell.rs`'s `cmd_backupsched` `list` subcommand.

**The defect, in two layers.**

1. *Dead configuration.* `BackupSchedule::frequency` had a `label()` and
   nothing else. No caller in the tree computed due-ness from it. The
   companion state was equally inert: `last_run_ns` was written by
   `run_now` and read by no one, in this module or any other.
2. *In-band sentinel.* `last_run_ns: u64` used `0` for "never ran". `0` is
   a legal uptime, so once a due-check existed it would have read a
   never-run schedule as one that ran at boot. The failure direction is the
   bad one: the schedule that had never once run is the one the scheduler
   would have been most confident it could skip, for a full interval after
   every reboot.

**The fix.** `BackupFrequency::interval_ns() -> Option<u64>` (`None` for
`Manual`, which genuinely never comes due on its own and so is not
expressible as a large interval); `last_run_ns: Option<u64>`; and
`due_schedules()` returning the enabled schedules whose interval has
elapsed or which have never run. `bsched list` now shows the last-run time
and a `DUE` marker, so the state is observable rather than merely stored.

**Note on scope.** `interval_ns` treats a month as a flat 30 days. The
kernel has an uptime counter, not a calendar, so "the same day-of-month
next month" is not expressible here. Documented at the definition.

**Not yet done:** nothing *calls* `due_schedules()` on a timer yet — this
commit makes the frequency meaningful and observable, but automatic
triggering needs a periodic task, which belongs with `tasksched`
integration. Tracked below.
