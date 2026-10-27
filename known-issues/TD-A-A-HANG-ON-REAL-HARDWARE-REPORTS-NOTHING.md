## TD-A-A-HANG-ON-REAL-HARDWARE-REPORTS-NOTHING (lane A, 2026-10-27) — OPEN

**Status:** OPEN.

**In short:** when the kernel stops on a real PC with interrupts off -- a
driver spinning on a register, an interrupt handler that never returns --
nothing says where. Under QEMU an emulated watchdog card turns such a hang
into a report of the stuck instruction (`kernel/src/hardlockup.rs`); a real
PC has no such card, so the screen just stops changing. The test PC's boots
on 2026-10-27 stopped twice that way, soon after the USB controller started,
and the place could only be guessed from the last log line.

**Where:** `kernel/src/hardlockup.rs` (the i6300esb watchdog, QEMU only);
`kernel/src/idt.rs`'s NMI handler, which already prints the stuck RIP, the
task and a backtrace.

**The proper fix:** a watchdog every x86 PC has -- the CPU's own performance
counter. Count unhalted core cycles in a general counter (IA32_PERFEVTSEL0,
event 0x3C), preloaded to overflow every second or two, the local APIC's
performance-counter LVT entry set to deliver an NMI. In the NMI: re-arm and
unmask the LVT entry; if the timer tick has not advanced since the last NMI,
report the hang through the existing NMI report (which reaches the screen
under `log.screen=1`). A CPU halted in `hlt` counts no cycles and is never
reported, which is right: it is waiting, not stuck. A wait that never ends
with interrupts on is the timer-driven detectors' to report.

QEMU's software CPU emulates no performance counters, so it can be tested
only on the test PC or under KVM.
