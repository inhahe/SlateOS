## A-DISKS-WRITE-CACHE-FLUSH-TIMES-OUT-AT-SHUTDOWN-ON-THE-TEST-PC (lane A, 2026-11-06) — OPEN

**Status:** OPEN

**In short:** when the test PC shut down at a press of its power button,
SlateOS asked every disk to write out its cache, as it must before the power
goes, and one disk did not answer in time: the kernel said "flushing a
disk's write cache failed (TimedOut); switching anyway" and powered off. On
the spare disk, which the tests may write, a cache not written out is data
lost; on the Windows disk, which nothing writes, it costs nothing. Which
disk it was, the line does not say. QEMU's disks have never shown it.

**Where.** `kernel/src/power.rs`, `flush_filesystems`, called by
`power::shutdown` -- here from the power button's worker
(`acpi::events::pressed`, a work-queue task) -- calls
`kernel/src/blkdev.rs`'s `flush_all`, which flushes every registered disk
in turn, holding the registry's lock throughout, and returns the first
error alone. Seen in the serial log of 2026-11-06 (`build/pikvm-serial.log`
until the next capture), line 1138, between "[power] Initiating system
shutdown..." and "[power] Trying ACPI S5": the PC's disks are two NVMe
drives -- `nvme0n1` (980 PRO, read-only to SlateOS) and `nvme1n1` (960
EVO, writable) -- and an AHCI controller with none.

**How to reproduce.** Boot the test PC with `log.pci_serial=1`
(`pikvm.py boot --serial`), press its power button (`pikvm.py power off`),
`pikvm.py serial fetch`.

**The proper fix.** First, say which disk and which command: `flush_all`
naming each disk whose flush failed, and the NVMe driver saying what it was
waiting for (the FLUSH's completion, the queue it was on, how long it
waited). Then the cause -- a FLUSH to the read-only disk the driver should
not send, a completion the driver misses when it comes from a work-queue
task rather than a process, a timeout too short for a real disk's cache
-- fixed where it lies.
