## BUG-C-BACKUP-SCHEDULE-WRITES-A-FILE-NOTHING-EVER-READS

> **Status 2026-09-27: lane E's half FIXED; waiting on lanes D and B.** The
> operator answered C-Q21 (design-decisions §1426): a service started at
> boot runs the backups, and one missed while the machine was off runs as
> soon as it is on again, without asking. The backup program now keeps
> every schedule in `<config>/backup/schedules.json` (not in each
> destination, where nothing could find them) with a time and a day, and
> `backup run-due` runs whatever is due and records it -- so the service
> only has to run that command, as each user, at boot and every few
> minutes (`requests/e-db-the-backup-service-runs-backup-run-due.md`).
> Until it exists, `backup schedule` says that nothing runs it on its own.

**Date:** 2026-09-16. **Lane:** C.
**Where:** `apps/backup/src/main.rs` — `cmd_schedule` (~2652), `schedules_path`
(~1805).

**In short:** the backup program offers to run a backup every day, week or
month. Nothing in the whole operating system ever runs one. The command writes
the schedule to a file and reports success, and that file is read by no
program that exists — there is no timer, no service and no check at start-up
that anything is due. A user who sets a daily backup and walks away has no
backups at all, and nothing ever tells them so.

**The evidence.** `schedules.json` is named in exactly four places in the tree,
all inside `apps/backup`: the function that builds its path, the write in
`cmd_schedule`, a comment about how it is written, and two assertions in that
command's own test. There is no reader anywhere — not in `apps/backup`, not in
`services/`, not in the shell.

```
backup schedule --source /home/me --dest /mnt/disk --interval daily
  -> writes schedules.json, prints success
  -> no program reads schedules.json, ever
```

**Why this is a bug and not a missing feature.** The command succeeds. A
missing feature is one the user cannot ask for; this is one they *can* ask for,
are told they have got, and have not got. It is the shape design-decisions 856
is about, in the most costly place it can appear: the claim is "your data is
being copied", the reality is that it is not, and the discovery comes when
something is already lost.

The operator named this exact failure when answering Q46: *"'periodic' needs a
trigger nobody has defined; in practice it tends to mean 'never'"*, with the
instruction *"Make a solution that will not result in 'never' in practice."*
That answer was about benchmark profiles, so the remark was incidental there —
but it describes this precisely.

**One observation rescued from the deleted shell panel, because it is about
this.** `gui/desktop/src/backup_settings.rs` recorded `last_backup_timestamp`
from its history and never displayed it. The note in
`scripts/fields-written-never-read-baseline.txt` put the point better than the
code did: *"the one question a backup screen exists to answer -- am I backed
up? -- is the one it does not answer."* The panel is gone and the baseline
entry with it, but the requirement outlives both: whatever eventually shows
backup state must lead with when the last one actually ran, not with what is
scheduled. A schedule is a promise and a timestamp is evidence, and this bug is
what happens when a program offers the first in place of the second.

**What the fix needs, and why it is not a one-liner.** Something has to run
when the machine is idle or at start-up, notice a schedule is due, and run the
backup. There is no cron, no timer service and no user-level scheduler in the
tree. Three candidate homes, none obviously right:

| Home | Fits because | Does not fit because |
|---|---|---|
| a service under `services/` | a backup should run whether or not anyone is logged in | `services/**` is lane B's tree |
| the shell (`gui/desktop`) | it already starts at login and already has an idle watch, which is a timer that works | no user logged in means no backup, and the shell is the wrong place to own data safety |
| `apps/backup` itself, re-run on a trigger | keeps the logic where the knowledge is | still needs something to do the triggering |

**The honest interim, which is in lane and worth doing regardless of which home
wins:** `cmd_schedule` should say what it does. It records a schedule; it does
not cause a backup. `apps/netmanager` already sets the precedent of refusing
out loud rather than appearing to work, and `posix`'s `require_shell` is the
in-tree model for a stub that documents its own emptiness rather than
pretending. Silence here is the part that turns a missing feature into a bug.
