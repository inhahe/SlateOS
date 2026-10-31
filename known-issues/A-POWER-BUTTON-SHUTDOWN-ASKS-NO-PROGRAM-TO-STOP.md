## A-POWER-BUTTON-SHUTDOWN-ASKS-NO-PROGRAM-TO-STOP (lane A, 2026-10-31) — OPEN

**Status:** OPEN

**In short:** when the power button (or the kernel shell's `initproc
shutdown`) shuts SlateOS down, filesystems are synced and logs flushed
before the power goes, but no running program is asked to save its work
and stop. A program holding unsaved data in its own memory loses it, as
in a power cut, though everything it had already written reaches the disk.

**Where.** `kernel/src/initproc.rs` (`shutdown`): its "stop services"
phase calls `fs::servicemgr::stop_service`, which only marks the
kernel's service table stopped (its process id 0; servicemgr's own
DEFERRED note). No process is sent anything or waited for.
`kernel/src/acpi/events.rs` (`pressed`) calls it, then powers off.

**The proper fix.** The shutdown belongs to userspace init (lane D's
`services/init`): on a press, the kernel sends init an IPC message (the
spec allows no Unix signals for process control). Init asks each service
and session to stop, by message, in reverse dependency order. It waits a
bounded time, ends what remains, and only then asks the kernel to sync
and power off (`reboot(2)` with POWER_OFF). The kernel's own path stays
the fallback for when no init answers. This needs a request to lane D for
init's side, and a kernel channel to deliver the press.

Trigger: init gaining a shutdown sequence, or the first userspace program
that holds data a power-off would lose.
