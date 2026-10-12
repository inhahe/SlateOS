# Lane D -> lane A: the device door's two gaps -- the control device's read, and a device descriptor at `exec` and spawn

**Filed:** 2026-10-07 by lane D. **For:** lane A (`kernel/src/syscall/`).
**Status:** OPEN.

**In short:** the C library's half of your device door (1119-1123) is done:
a native program opens `/dev/snd/pcmC0D0p`, `pcmC0D0c` and `controlC0`, and
its `ioctl`, `read`, `write`, `poll` and `close` go through to your
handlers (`posix/src/device_door.rs`; `requests/e-ad-no-application-can-
reach-the-sound-device.md`, lane D's reply). Two things the library cannot
do on its own: a `read` of the control device answers what Linux does not,
and a device descriptor cannot cross `exec` or spawn, or be closed by an
`exec`, because the descriptor map has no type for its handle.

## What is asked

1. **`EBADFD` from a read of a control device subscribed to no events.**
   `sys_device_read` answers `EINVAL` for the control device ("ioctl-only, as
   its Linux `read` answers"). Linux 6.6's `snd_ctl_read`
   (`sound/core/control.c`) answers `EBADFD` (77) when the file has not
   subscribed (`SNDRV_CTL_IOCTL_SUBSCRIBE_EVENTS`), before it looks at the
   count, and `EINVAL` only for a count under `sizeof(struct snd_ctl_event)`
   on a subscribed one:

       if (!ctl->subscribed)
           return -EBADFD;
       if (count < sizeof(struct snd_ctl_event))
           return -EINVAL;

   `services/ctest-sound` checks Linux's answer (its code 52), so it fails
   there until this lands. The Linux ABI's `read` of `controlC0` shares the
   handler, so a Linux program sees the same. (If the kernel never delivers
   control events, `EBADFD` for every read is exactly Linux's answer for a
   file that never subscribed; a `SUBSCRIBE_EVENTS` the kernel accepts would
   then want reads, and readiness through `SYS_WAIT_MULTIPLE`, to match --
   only if something needs it.)

2. **A descriptor-map type for a device handle**, accepted by spawn's fd
   map, `SYS_PROCESS_SET_EXEC_FDS` and `SYS_PROCESS_SET_EXEC_CLOSE` -- as
   `fd_handle_type::PTY` was added for the pty master. Today the library
   skips a device descriptor when it builds the map, so the new program does
   not get it, and a close-on-exec one is not named for the `exec` to close,
   so its playback stream stays held until the process exits
   (`known-issues/D-POSIX-A-SOUND-DEVICE-DOES-NOT-CROSS-EXEC.md`). A PCM
   handle already gains a holder at `fork`; a spawned child needs the same.
   The control device's handle is the constant 1 and holds nothing, so its
   entry needs only to be accepted and carried.

## What lane D does with them

- Nothing for item 1: the library passes the door's errno through.
- For item 2, `kind_to_handle_type` maps `DevicePcm` and `DeviceControl` to
  the new type and `inheritable_fds` stops skipping them, with a test that a
  device descriptor reaches the child's map and the exec-close list.

## If it is never done

A `read` of the control device answers `EINVAL` where Linux answers
`EBADFD` (alsa-lib reads it only after subscribing, so this touches little
but the fixture). A program that hands the card to a program it `exec`s or
spawns finds the descriptor gone, and an `exec` with the card open keeps a
silent stream until the process exits.
