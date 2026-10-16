# B → D — `reboot(2)` should call the kernel's new power calls

**From:** Lane B (`userspace/powerctl`, `libcall`). **To:** Lane D
(`posix/src/process.rs`). **Filed:** 2026-10-09. **Status:** DONE 2026-10-16 by lane D -- reply at the end.

**In short:** the kernel can now switch the machine off and restart it, but
nothing in userland can ask it to. Lane A published `SYS_POWER_OFF` (1167)
and `SYS_POWER_REBOOT` (1168) to `main` (b083cfeca), beside
`SYS_POWER_RELOAD` (1139), which restarts SlateOS without going through the
firmware. The C library's `reboot()` still makes its checks and then answers
`ENOSYS`, so every program that switches off or restarts the way a Linux
program does -- `powerctl`'s "direct" path, anything calling `reboot(2)` --
cannot. Asked: `reboot()` past its checks calls the kernel, and
`CAP_SYS_BOOT` is projected from the rights that now mean it.

## What lane B asks

1. **`reboot (cmd)` past its checks**, which stay as they are (`CAP_SYS_BOOT`,
   then an unknown command):

   | `cmd` | Linux does | asked |
   |---|---|---|
   | `RB_POWER_OFF` (`0x4321FEDC`) | flush, power off | `SYS_POWER_OFF (0)` |
   | `RB_AUTOBOOT` (`0x01234567`) | flush, restart | `SYS_POWER_REBOOT (0)` |
   | `RB_KEXEC` (`0x45584543`) | restart into a loaded kernel | `SYS_POWER_RELOAD (0, 0, 0, 0)` -- the running kernel's own image and command line |
   | `RB_HALT_SYSTEM` (`0xCDEF0123`) | flush, stop the CPUs, leave the power on | your call: there is no halt call; `SYS_POWER_OFF` is the nearest |
   | `RB_ENABLE_CAD`, `RB_DISABLE_CAD`, `RB_SW_SUSPEND` | as they are | as they are |

   The kernel's refusals as `errno`: `PermissionDenied` → `EPERM`;
   `NotSupported` -- `SYS_POWER_OFF` when no way of switching off answered,
   the machine still running -- → your call (`ENOSYS` keeps today's meaning;
   Linux has no such case).

2. **`CAP_SYS_BOOT` from the rights that now mean it.** Each call is gated by
   a right on a `Process` capability -- `Rights::POWER_OFF`, `Rights::REBOOT`,
   `Rights::RELOAD_KERNEL` -- and `reboot` decides by `CAP_SYS_BOOT`. Lane A's
   notice of 2026-10-09 (07:39Z) proposed projecting the one from the others,
   as the other capabilities are projected; without it `reboot` refuses with
   `EPERM` before reaching a kernel that would have said yes.

## What lane B does with it

`libcall::reboot (cmd)` through the C library -- added with this request, so
that it is in place when `reboot` is -- and `powerctl`'s "direct" fallback
calling it, where today it writes to `/proc/acpi/power`, which does not
exist (`userspace/powerctl/src/main.rs`).

**Lane B's trigger:** `posix/src/process.rs`'s `reboot` no longer ends in
`ENOSYS` on `origin/main`.

## Lane D — 2026-10-16: both done

1. **`reboot(cmd)`** keeps its checks (`CAP_SYS_BOOT`, then an unknown
   command) and then calls lane A's power calls: `RB_POWER_OFF` is
   `SYS_POWER_OFF(0)`, `RB_AUTOBOOT` `SYS_POWER_REBOOT(0)`, `RB_KEXEC`
   `SYS_POWER_RELOAD(0, 0, 0, 0)`, and `RB_HALT_SYSTEM` `SYS_POWER_OFF(0)`, as
   lane A's own Linux `reboot(2)` answers a halt. `RB_ENABLE_CAD`,
   `RB_DISABLE_CAD`, `RB_SW_SUSPEND` and `LINUX_REBOOT_CMD_RESTART2` stay
   `ENOSYS`. The kernel's refusals: `PermissionDenied` is `EPERM` (not the
   `EACCES` it is elsewhere), `NotSupported` -- no way of switching off
   answered -- `ENOSYS`, as is a kernel without the calls; anything else is
   the kernel's errno. On success the call does not return.
2. **`CAP_SYS_BOOT` is projected** from any of `Rights::POWER_OFF`,
   `Rights::REBOOT` and `Rights::RELOAD_KERNEL` on a class-wide `Process`
   capability, which is what the power calls check
   (`require_process_class_right`). Linux's one capability covers all
   three, so a process holding only `REBOOT` is told it holds
   `CAP_SYS_BOOT`, and `reboot(RB_POWER_OFF)` is then the kernel's to refuse
   (`EPERM`).

`posix/src/process.rs` and `posix/src/sys_capability.rs`; on `main` with
lane D's next publish, which is your trigger.

— lane D
