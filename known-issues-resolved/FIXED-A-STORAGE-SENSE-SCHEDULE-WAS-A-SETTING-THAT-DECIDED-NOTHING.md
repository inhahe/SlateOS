### FIXED-A-STORAGE-SENSE-SCHEDULE-WAS-A-SETTING-THAT-DECIDED-NOTHING (lane A)

**In short:** Storage Sense is the automatic disk-cleanup feature. You
could set it to "Daily", "Weekly", "Monthly" or "when the disk gets
full", and the setting would be remembered and shown back to you forever
— but nothing in the system ever looked at it. Cleanup ran only when you
asked for it by hand. Fixed in `78cae81f5`: the schedule now determines
whether cleanup is due, and `storagesense show` says so.

**Status:** FIXED (`78cae81f5`).

**Where:** `kernel/src/fs/storagesense.rs`.

**What was wrong.** Three fields — `schedule`, `low_space_threshold_mb`
and `last_run_ns` — were all written and none of them ever read. Two of
the three were settable from the shell (`storagesense schedule daily`,
and the threshold), so the feature presented a full configuration
surface over machinery that did not exist. Dead configuration is worse
than absent configuration: absent configuration tells the user to do it
themselves, dead configuration tells them it is handled.

**The second bug, hidden behind the first.** `last_run_ns` was a `u64`
in which `0` meant "never run". The clock is nanoseconds-since-boot and
starts *at* zero, so `0` is a legal instant. This could not misbehave
while nothing read the field — and giving it a reader is precisely what
would have armed it. A cleanup run that landed at uptime 0 would read
back as never having run, so the new due-check would have declared
cleanup due again immediately and deleted the same files a second time,
on the one boot where the first pass had only just finished. It is now
`Option<u64>`.

**Note on the low-space path.** `due_reason()` releases the module lock
before calling `Vfs::statvfs`, because `statvfs` descends into a
filesystem driver and may take VFS locks of its own. A `statvfs` that
fails yields "not due", never "low space": the remedy for low space is
deleting the user's files, so an unknown free-space figure must not be
treated as a low one.

**Not yet done:** as with backup schedules, nothing polls `due_reason()`
on a timer — see `TD-A-BACKUP-DUE-SCHEDULES-HAS-NO-PERIODIC-CALLER`,
which is now the same gap in two modules and wants one `tasksched` task
that services both.
