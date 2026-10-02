## TD-C-A-DEVICE-MANAGER-THAT-INVENTED-THE-MACHINE -- FIXED 2026-09-15

**In short:** `apps/devicemanager` listed the computer's hardware -- graphics
card, IRQ numbers, memory ranges, driver versions and dates -- and had no way
to look at any of it. Every device it showed was marked Working.

**Date:** 2026-09-15. **Lane:** C.

The crate has no `/sys` reader, no `std::fs` and no syscall that could
enumerate a bus. `sample_devices()` supplied the tree: a Virtio GPU at IRQ 11
with an MMIO range of `0xFD000000`-`0xFDFFFFFF`, driver `virtio-gpu` version
`1.2.0` dated `2026-04-01`, and several more, each with a hardware ID, a
vendor, a status and a driver date. `sample_events()` supplied a matching
event history.

**What makes a device manager a uniquely bad host for this is who opens one.**
It is where somebody goes *when hardware is not working*. The thing they came
to find is precisely the device that is missing, faulted, or claimed by the
wrong driver -- and **an invented inventory cannot be missing anything**. Every
device in it was Working. So the program answered the one question it exists
to answer, wrongly, for the reader least able to discount the answer.

**Six acts it could not perform.** Scan (documented as "Scan for hardware
changes") recomputed the tree from a list nothing changes, and reported the
non-search by the strongest means available: leaving the list exactly as it
was, so the user concluded nothing had changed. Enable and Disable flipped a
flag in this process. **Uninstall is the worst**: somebody troubleshooting
removes a driver, sees it gone, and reboots expecting the system to install a
fresh one -- nothing was removed, so nothing is reinstalled, and they have now
*eliminated* the real fault from their search. Export ended with
`let _report = self.export_report();` and a comment saying a real app would
save it: a button that discarded its own output in silence.

**Export now builds the report anyway and says how many bytes it would have
written.** That is the difference between "cannot write a file" and "nothing
happened", and the report's own field sanitiser -- which stops a
hardware-supplied string from redrawing the table -- stays exercised on the
production path rather than only from tests.

**The refusals in Enable, Disable and Uninstall are written out even though
nothing can currently reach them**, because `selected_device()` has nothing to
select. An empty list is a reason, not a safeguard, and it stops being empty
the day a real enumerator lands.

**The banner is keyed on the device list being empty, not on a constant.** It
disappears by itself when something fills the list, rather than becoming a
stale claim of its own -- which is the failure mode three descriptions in
`apps/benchmark` hit earlier the same day.

**When a real producer arrives it is `/sys/devices`**, the same source
`sysinfo` reads since design-decisions §850. Lane A has
`block/<name>/{sector_count,sector_size,read_only}` built and pending a green
boot.

**51 of 137 tests rested on the invented inventory** -- the tenth application
in a row. Most were about the tree, the search filter, the properties tabs,
resource conflicts and the report, all of which need devices rather than
specifically invented ones, and moved to a `with_sample_devices()` fixture.
Three asserted the fabricated acts, including `test_state_default`, which
asserted that a freshly opened manager already knew about hardware it had
never looked for.
