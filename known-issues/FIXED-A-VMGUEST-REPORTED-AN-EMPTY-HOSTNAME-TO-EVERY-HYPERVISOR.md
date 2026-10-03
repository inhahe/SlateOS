### FIXED-A-VMGUEST-REPORTED-AN-EMPTY-HOSTNAME-TO-EVERY-HYPERVISOR

**Date:** 2026-08-23 · **Lane:** A · **Where:** `kernel/src/vmguest.rs`
(`GuestInfoData::hostname`, `guest_info()`, `procfs_content()`, and tests 3
and 10 of `self_test_inner`)

**In short:** the kernel tells the hypervisor what machine it is — OS name,
version, CPU count, memory, hostname. The hostname it sent was always blank,
because the field holding it was a private second copy that startup never
filled in. Nothing noticed, because the one piece of code that ever wrote
that field was the module's own test, which wrote it, checked it, and then
"restored" it to a value nothing had ever put there.

Boot batch 26 panicked at `vmguest.rs:1346`, `Should be initialized when in
VM`. That was the test bug; the hostname was the product bug hiding behind
it.

**The test bug.** Test 3 called `stats()` and asserted `initialized`. That
held only because the suite ran against live state, so it was observing the
`init()` the boot sequence had called minutes earlier. It asserted that
*something else* had run. Once the module was de-fanged — pristine `State`,
`INITIALIZED` pinned to its initialiser `false` — there was no longer any
such residue to observe, and the assertion failed correctly.

The fix is to call `init()` and *then* assert, which is what the test should
always have done: exercise the function rather than watch for its leavings.
`init()` is safe inside the pristine window because it writes only `STATE`,
`INITIALIZED` and `ACTIVE_FEATURES`, all three of which `self_test` saves,
and its inputs (hypervisor detection, CPU count, frame stats) are read-only.

**What that uncovered.** Tests 7, 8 and 9 are gated on `initialized` and on
feature support. Against an uninitialised module all three evaluated their
guard, took the else branch, and printed "skipped" — so five of the suite's
thirteen tests had quietly stopped testing anything. Restoring `init()`
turned them back on, and test 7's `assert!(!hostname.is_empty())` failed on
the first real run it had ever had.

`init()` populates `os_name`, `os_version`, `kernel_version`, `cpu_count` and
`total_memory`. It does not populate `hostname`, and never did. `State::new`
— which is the `static`'s own initialiser — leaves it `String::new()`. So on
every real boot, `guest_info()` and `/proc/vmguest` reported `hostname: `.

The field survived because the self-test maintained it. Test 10 called
`set_hostname("test-host")`, asserted it had taken, then called
`set_hostname("mintos")` under a `// Restore.` comment. That line restored
nothing — it was the first and only write of a plausible value, and it made
the module look correct for as long as anyone was watching.

**Fix.** There is one system hostname and it already lives in `fs::sysfs`,
behind `/sys/kernel/hostname`, with a `"mintos"` fallback. `vmguest` now
reads it through `fs::sysfs::get_hostname()` at the moment it reports, and
the private field and its `set_hostname` are gone. The read happens *before*
`STATE` is locked, so neither function establishes a lock ordering between
the two. Test 10 now asserts the invariant that replaced the field — what
`/sys/kernel/hostname` holds is what leaves this module, through both
`guest_info()` and `/proc` — which is also non-destructive, which the test it
replaces was not.

`GuestInfoData::new` was deleted in the same change: nothing called it, and
it had drifted from `State::new` in three of its six fields (`os_name`,
`os_version`, `hostname`), exactly the dead-constructor drift already
documented on `State::new`.

**The lesson, which is the same one twice now.** A duplicate that only the
test maintains is worse than no duplicate at all: it makes the test pass and
the product wrong, and it does so in the direction nobody checks. And a
conditional test whose guard can go false is a test that can stop existing
without ever reporting that it did — tests 7-9 printed "skipped" in a log
nobody diffs. Prefer a guard that is *asserted* to hold over one that merely
selects a branch.
