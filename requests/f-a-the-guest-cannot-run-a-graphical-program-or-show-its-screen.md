# F → A — the guest agent cannot start a graphical program, and the guest has no way to show its screen

**From:** Lane F (`gui/compositor`). **To:** Lane A (`kernel/src/proc/spawn.rs`'s
`ctest_generic_grant`, `scripts/guest.py`). **Filed:** 2026-10-10.
**Status:** OPEN.

**In short:** to get the desktop running on SlateOS, lane F needs to run the
compositor inside a guest (`scripts/guest.py`, design-decisions 1534), look at
what it puts on the screen, and press a key at it. Two things stop that today.
The agent can only grant `file`, `secureboot` and `reload_kernel`, and the
compositor needs a socket to listen on -- without one it exits at once -- and
the input devices. And `guest.py`'s QEMU shows nothing (`-display none`), has
no monitor, and has no keyboard or mouse, so nothing outside the guest can see
the screen or type at it. The compositor itself builds for SlateOS
(`cargo +nightly build-slateos -p compositor --release`, two minutes) and the
guest has `/dev/dri/card0` and `/dev/input/event0`-`1`.

## What is asked

1. **Three grant words in `ctest_generic_grant`**, which are what the
   compositor's `/etc/startup.conf` line names
   (`requests/f-bd-the-display-service-needs-two-grants-and-a-flag-from-the-session.md`),
   so a program tried in a guest holds what it will hold at boot:

   | Word | Grant | The compositor needs it to |
   |---|---|---|
   | `socket` | `(Socket, 0, READ \| WRITE)` | listen on `127.0.0.1:7373` for its clients |
   | `input` | `(InputDevice, 0, READ)` | read `/dev/input/eventN` |
   | `service` | `(Service, 0, WRITE)` | offer `org.slateos.Display` |

2. **Options for `guest.py start`:**
   - `--qmp PORT`: `-qmp tcp:127.0.0.1:PORT,server=on,wait=off`. With it the
     host can `screendump` -- the scanout as QEMU shows it, the only view of
     the screen nothing inside the guest can fake -- and `input-send-event`.
   - `--input`: `-device qemu-xhci,id=xhci0 -device usb-kbd,bus=xhci0.0
     -device usb-mouse,bus=xhci0.0` (the kernel's HID driver takes boot
     keyboards and mice; a `usb-tablet` it would not).
   - `--display WHAT`, default `none`: `gtk` or `sdl`, so a person can watch
     the guest and type at it.

   If one mechanism suits `guest.py` better than three, a repeatable
   `--qemu-arg ARG` that passes arguments through would do: lane F would
   then keep the device list and the QMP client in a script of its own.

3. **Optional:** an agent request that starts a program and answers at once
   with its pid, for a program meant to keep running -- a compositor that
   clients then connect to. Meanwhile `run --seconds N` serves, with the
   screendumps taken from the host while it runs.

## What lane F does with it

A script that starts a guest with these, puts the compositor and a client
program in, takes a screendump, and compares it with what the compositor's
host build composites for the same client; then fixes whatever breaks in the
compositor's DRM and evdev paths, which have never run on SlateOS.
