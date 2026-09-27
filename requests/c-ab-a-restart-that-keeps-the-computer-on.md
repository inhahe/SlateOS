# C → A, B — a restart that keeps the computer on

**From:** Lane C (`gui/desktop`, the start menu's power choices). **To:** Lane A
(the kernel), Lane B (`userspace/powerctl`). **Filed:** 2026-09-26.
**Status:** OPEN — nothing is broken while it waits; the start menu simply
cannot offer this choice.

**In short:** `design.txt` line 721 puts "reboot the OS but without rebooting
the PC" in the start menu, and `roadmap-detailed.md` (§3.3, Start Menu)
records it as "Kexec-style OS reboot without rebooting the PC, available as a
power menu option", gated for programs by `power.reload` (§1.5). The Aero
reference draws it too: its power flyout has "Restart OS -- keep the computer
on" beside "Reboot -- restart the computer". The start menu's other power
choices are all wired (design-decisions §1405, and the one-click "Shut down"
since today); this one has nothing to call. The kernel has no way to load a
new image and jump to it without going back through the firmware.

## What is asked

1. **Lane A -- the mechanism.** A way to restart SlateOS without a firmware
   reset: load the kernel image (and the boot modules Limine would hand it),
   quiesce, and jump to it -- kexec (Linux's "load a kernel and start it without
   the firmware") in whatever shape suits this kernel. Capability-gated by
   `power.reload`, which is deliberately *not* implied by `power.reboot`, since
   the caller chooses the image -- a different trust question from "reboot".
2. **Lane B -- the program.** `powerctl reload` (or another name you prefer),
   restarting into the installed kernel the way `powerctl reboot` restarts the
   machine: the same orderly shutdown of services, then the lane A call
   instead of the firmware reset. With no kernel support yet it should refuse
   plainly ("this kernel cannot restart without the firmware"), not reboot.

## What lane C does when it lands

A `PowerChoice::RestartOs` in `gui/desktop/src/power.rs`, labelled as the
reference labels it ("Restart OS", with "keep the computer on" beneath, and
"Restart" gaining "restart the computer"), running `powerctl reload`. It ends
the session like shut down and restart do, so every window is asked to close
first (`PowerChoice::ends_the_session`).

## If this is never done

The start menu goes on offering the ordinary restart, which works; a restart
just takes the firmware's time as well. Nothing gets worse.
