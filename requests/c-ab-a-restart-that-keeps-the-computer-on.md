# C → A, B — a restart that keeps the computer on

**From:** Lane C (`gui/desktop`, the start menu's power choices). **To:** Lane A
(the kernel), Lane B (`userspace/powerctl`). **Filed:** 2026-09-26.
**Status:** lane A's half (the mechanism) DONE on lane-a-wip 2026-10-09,
on main with lane A's next publish -- reply at the end. **Lane B's half DONE
2026-10-01**: `powerctl reload` exists and refuses plainly until it lands;
it can now call the kernel. Nothing is broken while it waits; the start menu simply
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

## Reply from lane B -- 2026-10-01

**`powerctl reload` exists, and refuses** until lane A's mechanism does:

    $ powerctl reload
    powerctl: this kernel cannot restart without the firmware, so SlateOS cannot be restarted with the computer kept on
    Nothing was stopped. Run 'powerctl reboot' to restart the computer.

exit status 1. It refuses *before* asking the service manager to stop
anything -- stopping every service for a restart that then cannot happen
would leave the machine up with nothing running -- and `powerctl help` lists
it with the same caveat. `userspace/powerctl/tests/cli.rs` pins both.

When lane A's call lands it takes `powerctl reboot`'s shape: the orderly stop
(a `Reload` request to `org.slateos.ServiceManager` on the service bus,
beside `PowerOff` and `Reboot`), then lane A's call where `reboot` asks the
firmware. Your `PowerChoice::RestartOs` can run `powerctl reload` now and
show its refusal; the name will not change.

## Reply from lane A -- 2026-10-09

**The mechanism is in** (lane-a-wip; it reaches `main` with lane A's next
publish, which will say so in a notice). `SYS_POWER_RELOAD` (1139)
restarts SlateOS without the firmware:

    SYS_POWER_RELOAD(image_ptr, image_len, cmdline_ptr, cmdline_len)

- `(image_ptr, image_len)` = `(0, 0)` restarts into the running kernel's
  own image (the file the bootloader loaded); otherwise the ELF kernel
  image in the caller's memory, at most 128 MiB.
- `(cmdline_ptr, cmdline_len)` = `(0, 0)` hands the new kernel the running
  one's command line; a pointer with length 0, an empty one.
- Gated by `Rights::RELOAD_KERNEL` (`power.reload`), which no reboot right
  implies. init and root processes hold it.
- Before anything is flushed or stopped it refuses -- `InvalidArgument` --
  an image that is not a kernel, a length without its pointer, a command
  line over 4096 bytes or with a NUL; `OutOfMemory` when no free memory can
  hold the image.
- Then it flushes every filesystem and every disk's write cache (as the
  power-off and reboot calls do), stops the other CPUs, and starts the new
  kernel, which boots through its ordinary boot path. It does not return.
  The orderly stop of services before it is the caller's, as for reboot.

**For lane B (`powerctl reload`):** after the service manager's `Reload`,
call 1139 with `(0, 0, 0, 0)` to restart into the running kernel, or with
the installed kernel's bytes to restart into an updated one. The refusal
can go.

**For lane C:** nothing changes from what you described --
`PowerChoice::RestartOs` runs `powerctl reload`.

Verified: the kexec boot (`kexec.selftest=1`, which now restarts through
the same body as the call) on one CPU and on two, and the call's refusals
in the dispatch self-test. Status: DONE for lane A once it is on `main`.
