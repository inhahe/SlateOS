### [E] The Device Manager cannot say which driver runs a PCI device, nor its interrupt or memory -- 2026-09-26
**Status:** OPEN, waiting on lane A -- `requests/e-a-publish-each-pci-functions-irq-bars-and-driver.md`. The rest of the Device Manager's inventory is FIXED (lane E, 2026-09-26).

**In short:** the Device Manager now lists the machine's real devices -- where
it used to invent them (to 2026-09-15) and then show none (to today). But for
a PCI device it cannot say which driver runs it, which interrupt it uses or
which memory it occupies, because the kernel does not publish those. Every PCI
row says so in words ("Unknown" status, driver "Not reported"), and the
Resources view is empty.

**What is listed, and from where** (`apps/devicemanager/src/inventory.rs`,
through `apps/hwquery`, the reader System Information uses, so the two cannot
disagree about the machine): every PCI function from `/sys/devices/pci`,
sorted into a branch by its class code; each registered disk from
`/sys/devices/block`; each network interface but loopback from
`/proc/net/dev`; each display output from `/proc/monitors` (a disabled one as
Disabled); the processor. A source that cannot be read is named in the window
-- in the empty-tree banner, or beside the counts in the status bar -- so an
empty branch is not read as an absent device. The window scans once on
opening and again on F5. Export writes the report to a file the user picks,
through `safeio::write_str_atomically`.

**Wording changed with it.** "N/A" for an IRQ, a memory range, a DMA channel
or a hardware ID claimed the device has none; it is now "Not reported". "No
driver installed" claimed a finding about the machine; the Driver tab now says
the kernel does not report which driver runs a device. The status bar counts
the devices "with no status reported" where it counted "enabled", which every
listed device is in the only sense the kernel knows.

**Still refused, correctly:** Enable, Disable and Uninstall -- the kernel has
no interface for any of them. The refusal now meets real rows, which is the day
the 2026-09-15 entry (`TD-C-A-DEVICE-MANAGER-THAT-INVENTED-THE-MACHINE`) wrote
it for.

**Not listed at all:** USB devices behind a controller and sound devices as
such (the controllers appear as PCI functions). Nothing publishes either;
`/sys/hardware/usb` and `/sys/hardware/sound` never existed.
