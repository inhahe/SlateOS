### TD-A-STORAGE-SENSE-CLEANUP-IS-SIMULATED (lane A)

**In short:** Storage Sense reports freeing gigabytes, but it does not
actually delete anything — it copies each category's *estimated* size
into its "freed" figure. The numbers are made up, and always exactly
match the estimate.

**Status:** OPEN. Pre-existing; noticed while fixing the schedule.

**Where:** `kernel/src/fs/storagesense.rs::run_cleanup` and
`run_category` — `let freed = policy.estimated_bytes;` with the comment
`// Simulate cleanup: free the estimated amount.` The per-category
`estimated_bytes` values are themselves constants set in
`default_policies()`, not measurements.

**Why it matters more now.** Before `78cae81f5` the simulation could only
run when the user explicitly asked. Now that the schedule is meaningful,
the intended next step is a timer that runs cleanup automatically — and
wiring a timer to a function that reports fictional results would make
the fiction periodic and unattended. **The periodic caller must not be
added until the cleanup is real.**

**Proper fix.** Each category needs a real enumerate-and-delete pass
against the VFS: `TempFiles` over `/tmp`, `RecycleBin` via
`fs::recyclebin`, `Logs` over the log directory with `max_age_days`
actually applied, and so on. `estimated_bytes` should become the result
of a dry-run walk rather than a constant, so that estimate and result can
disagree — which is the whole point of having both.
