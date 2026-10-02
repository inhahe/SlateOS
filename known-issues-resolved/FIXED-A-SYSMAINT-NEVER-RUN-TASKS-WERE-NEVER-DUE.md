### FIXED-A-SYSMAINT-NEVER-RUN-TASKS-WERE-NEVER-DUE

**Status:** fixed. `MaintTask::last_run_ns` is `Option<u64>`; `None` is due.

**Symptom.** `sysmaint due` on a freshly booted machine listed **nothing**.
`sysmaint::self_test()` test 2 asserted 10 due tasks and got 0 — a boot-halting
panic at `kernel/src/fs/sysmaint.rs:355` the first time the suite ran at boot.

**Cause.** `last_run_ns: u64` used `0` as an in-band "never run" sentinel, and
`0` is a *legal* uptime — it means "at boot". `check_schedule` computed
`elapsed = hpet::elapsed_ns() - last_run_ns`, so for a never-run task that is
simply the uptime, and the task became due only once the machine had been up
for a whole interval. The seeded intervals are 12 to 720 hours, so:

| Task | Interval | First run required |
|---|---|---|
| Update Check | 12 h | 12 h of unbroken uptime |
| Index Rebuild, Temp Cleanup | 24 h | 1 day of unbroken uptime |
| Disk Trim, Cache Cleanup, Log Rotation, Security Scan | 168 h | 1 week |
| Integrity Scan, Performance Tune, Backup Verify | 720 h | **30 days** |

Nothing persists a run history across reboots, so on any desktop that is ever
rebooted or suspended, the monthly tasks would never have run once — not
"late", never. The scheduler was not slow; it was inoperative for its three
longest-interval tasks and unreliable for the rest.

**Fix.** `last_run_ns: Option<u64>`, with `None` meaning never and always due.
The in-band sentinel *was* the bug: nothing in the type said `0` was special,
so the arithmetic quietly treated it as data. `check_schedule` now branches on
the `Option` instead of subtracting. The interval multiply is `saturating_mul`
as well — `interval_hours` is a `pub` field and a wrapped interval would mean
"always due", failing in the direction that looks like it works.

Safe because nothing runs maintenance automatically: `run_pending` is reachable
only from the `sysmaint` shell command, so "every task is due at first boot"
cannot become "the monthly scan fires on every boot".

**What it cost, and why it hid.** The suite existed and asserted the right
thing. It had never run — it was a `sysmaint test` shell subcommand only, and
wiring it into the boot suite is what surfaced this. Second bug in this batch
found the same way, after the procfs self-deadlock.

**Related, not fixed:** the run history is in-memory only, so a reboot forgets
when maintenance last ran and every task is due again. Making that durable
needs a decision about wall-clock vs uptime as the stored basis (uptime is
meaningless across boots; wall-clock needs an RTC that may be wrong), so it is
left as tech debt rather than folded into a correctness fix.
