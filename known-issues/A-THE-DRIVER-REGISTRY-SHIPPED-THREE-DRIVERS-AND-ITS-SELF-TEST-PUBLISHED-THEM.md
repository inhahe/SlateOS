## A-THE-DRIVER-REGISTRY-SHIPPED-THREE-DRIVERS-AND-ITS-SELF-TEST-PUBLISHED-THEM (lane A, 2026-09-15) — **Status: FIXED**, pending a boot

**What it was.** `kernel/src/fs/driverupdate.rs::init_defaults()` seeded three `InstalledDriver` entries and `procfs::gen_driverupdate` published the count at `/proc/driverupdate` as `driver_count`. Nothing had installed anything.

| name | version | previous | status | provider |
|---|---|---|---|---|
| Virtual Display Driver | 1.2.0 | 1.1.0 | UpToDate | MintOS |
| HD Audio Driver | 2.0.1 | 2.0.0 | UpToDate | MintOS |
| Virtio Network Driver | 1.0.0 | — | **UpdateAvailable 1.1.0** | MintOS |

**Three things made it worse than `A-PROC-REPORTED-THREE-DEVICES-NOBODY-HAD-DETECTED`.**

1. **Asking created the answer.** `kshell`'s `cmd_driverupdate` list arm calls `init_defaults()` and *then* lists. An operator typing `driverupdate` to find out what was installed caused three drivers to come into existence and was shown them. The honest branch directly beneath, `if drivers.is_empty() { "No drivers registered." }`, could never run.
2. **The self-test published it on every boot.** `self_test()` is dispatched from `main.rs:7163` and calls `init_defaults()`, so the test was what populated the registry, and `/proc` then stated `driver_count: 3` for the rest of the run. The test created the claim and `/proc` repeated it.
3. **It was a narrative, not a number.** dmevent claimed three devices exist. This claimed software was installed, upgraded from a named previous version, and that an update was pending from a provider that publishes nothing. Lane C's distinction, from finding a fabricated IRC session the same day: an invented filename claims a file exists; an invented history claims events happened.

**Why the self-test was load-bearing.** It asserted `list_drivers().len() == 3` and drove install/rollback against seeded id 3, so the seeds looked required. They were not: a test that needs three drivers can register three. It now builds its own fixture with `register_driver`, named `Test Display` / `Test Audio` / `Test Network`.

**Removing the seeds was not sufficient**, because the test's own fixture lands in the same global registry `/proc` reads -- it would have reported `driver_count: 4` instead of 3, which is not an improvement. The registry had no removal path at all, a defect on its own terms: an uninstalled driver had no way to stop being reported. It gained `unregister_driver`, and the self-test now ends by removing everything it registered and asserting the count is zero, the way `fs::reclock`'s does. Zero is the honest answer to what is installed on this machine.

**How it was found.** Following lane C's triage of the same shape across ten apps. Not by a gate -- there is none for this, and `A-PROC-REPORTED-THREE-DEVICES-NOBODY-HAD-DETECTED` did not generalise into one. Both were found by reading a file that emits facts and asking where each value comes from. Third instance in one day, which is enough to make the read worth repeating even though it cannot be automated: *a plausible integer draws no reaction, which is exactly why it survives*.
