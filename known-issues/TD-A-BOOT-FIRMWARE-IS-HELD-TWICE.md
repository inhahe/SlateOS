## TD-A-BOOT-FIRMWARE-IS-HELD-TWICE (lane A, 2026-10-30) — OPEN

**Status:** OPEN

**In short:** The firmware a boot medium carries (design-decisions 1592)
stays in memory twice for as long as the system runs: once where the
bootloader loaded it, which the kernel never takes back, and once as the
read-only files placed at `/.boot-firmware`. Today that is the test PC's
WiFi firmware, about 1.6 MB, so 3.2 MB in all -- nothing on a machine with
gigabytes. It starts to matter with a GPU's firmware set, which runs to
tens of megabytes.

**Where.** `kernel/src/linuxdrv.rs`'s `place_firmware` copies each module
into a memfs file (`Vfs::write_file`); the module's bytes are Limine's
"executable and modules" memory, which `mm::frame` never adds to the free
frames (`kernel/src/boot.rs`, `MODULE_REQUEST`). After placement nothing
reads them but `boot_firmware_run`'s comparison in the self-test.

**The proper fix.** Either of:

- **Give the module's memory back once it is placed**: the frames of each
  firmware module's range, which Limine page-aligns, handed to the frame
  allocator after the copy -- but only frames no other module shares (a
  16 KiB frame may hold the end of one module and the start of the next,
  Limine aligning to 4 KiB), and with the self-test's comparison taken
  against a hash made at placement instead of the module.
- **A memfs file that adopts the module's frames** rather than copying
  them: memfs keeps a large file as frames already (`MemData::Frames`), so
  the frames wholly inside a module could be taken over, and only the
  partial ones at its ends copied.

**Trigger.** Do it before a driver whose firmware set passes ~16 MB is
staged by default -- the i915 run (roadmap 2.1, after WiFi) brings GuC,
HuC and DMC firmware -- or sooner if a machine with little memory is in
view. Found by the reviews of 2026-10-30 (Opus and Fable, the boot-firmware
batch).
