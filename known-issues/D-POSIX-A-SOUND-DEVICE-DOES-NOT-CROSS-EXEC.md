## D-POSIX-A-SOUND-DEVICE-DOES-NOT-CROSS-EXEC — a sound card descriptor is lost at `exec` and spawn, and a close-on-exec one is not closed by the `exec` (lane D, 2026-10-07)
**Status:** OPEN -- needs a descriptor-map type for a device handle from lane A (`requests/d-a-the-device-door-s-two-gaps.md`, point 2)

**In short:** a native program can now open the sound card
(`/dev/snd/pcmC0D0p` and the control device, through lane A's device door).
On Linux, a descriptor for it that is not close-on-exec survives `exec` and
is inherited by a spawned child, and one that is close-on-exec is closed by
the `exec`, which lets the card go. Here neither happens: the new program
does not have the descriptor, and the old one's handle -- a playback stream
-- stays held by the process until it exits. A program that opens the card
and then `exec`s another that plays to the inherited descriptor gets
`EBADF`; one that `exec`s with the card open keeps a silent stream alive.

**Where:**

- `posix/src/spawn.rs`, `inheritable_fds`: a device descriptor
  (`HandleKind::DevicePcm`, `DeviceControl`) is skipped, as a named Unix
  socket is, because the kernel's descriptor map (`SYS_PROCESS_SET_EXEC_FDS`,
  spawn's fd map) has no type for its handle; `kind_to_handle_type` maps it
  to `FILE` only so that the conversion is total.
- `posix/src/spawn.rs`, `exec_close_list`: built from the same map, so a
  close-on-exec device descriptor is not named for the kernel to close at
  the `exec` (`SYS_PROCESS_SET_EXEC_CLOSE`).

**Reproduce:** on a lane A kernel with a sound card, open
`/dev/snd/pcmC0D0p` without `O_CLOEXEC`, `exec` a program that `fstat`s the
same descriptor number: `EBADF`, where Linux answers a character device.

**The proper fix:** a descriptor-map type for a PCM handle (and one for the
control device, whose handle is the constant 1 and holds nothing), accepted
by spawn's map, `SYS_PROCESS_SET_EXEC_FDS` and `SYS_PROCESS_SET_EXEC_CLOSE`:
the kernel already counts a PCM handle's holders across `fork`, and a
spawned child needs one more holder as a forked one does. The library then
maps the two kinds to it in `kind_to_handle_type` and stops skipping them.
Asked of lane A in the request named above.
