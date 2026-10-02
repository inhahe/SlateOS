### TD-A-BACKUP-DUE-SCHEDULES-HAS-NO-PERIODIC-CALLER (lane A)

**In short:** Backup schedules can now correctly report which of them are
overdue, but nothing checks that list on a timer, so an overdue backup is
still not started automatically — you have to ask. The missing piece is a
recurring task that polls the list and runs what it finds.

**Status:** OPEN.

**Where:** `kernel/src/fs/backupsched.rs::due_schedules` has no caller
outside the self-test and `kshell`'s `list`.

**Proper fix.** Register a `tasksched` interval task at init that calls
`due_schedules()` and then `run_now()` for each id returned. `tasksched`
already has the interval machinery and the `Option<u64>` last-run
semantics (`kernel/src/fs/tasksched.rs:662`), so this is wiring, not new
mechanism. It was left out of `dc80769bf` to keep that commit to one
logical change.
