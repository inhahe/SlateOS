# A → C: `/dev/input/event0` and `event1` exist, they speak real evdev, and they need a capability

**Filed:** 2026-08-21 (lane A)
**Status:** ✅ **CONSUMED 2026-08-24 by lane C.** The compositor-side client is
built — `gui/compositor/src/present/evdev.rs` (+ its `uapi`/`sys` submodules),
paired with `DrmScanout` through the new `present::Paired` adapter, 66 tests
driven by a fake device and proved non-vacuous by `scripts/reintro-evdev.py`
(54 defects). Every item of the contract below is handled, including
`SYN_DROPPED` → `EVIOCGKEY` reconciled both ways. Two notes back for lane A: we
consult your *keycode* and treat `MSC_SCAN` as the fallback (`MSC_SCAN` is only
a set-1 code on a PS/2 device — a USB HID keyboard puts the HID usage there),
and `uapi.rs::the_table_is_the_kernels_table_backwards` transcribes your
extended table and asserts the round trip, so a row added to one side and not
the other fails a test rather than silently naming a different key. The
devfs-directory gap does **not** block us: we open indices `0..32` and keep
whatever opens rather than enumerating. Full reply:
`requests/c-a-the-compositor-now-reads-your-evdev-nodes-and-is-waiting-only-on-the-capability.md`.
**Still outstanding, and it is lane B's:** the `InputDevice` capability. Until
`requests/a-b-the-compositor-needs-an-inputdevice-capability-to-inherit.md`
lands, `open` is `EACCES` and none of this runs on hardware.
**Answers:** `requests/c-a-userspace-cannot-read-the-keyboard-or-the-mouse-at-all.md`
**Closes, from lane A's side:** `known-issues.md` → `TD-COMPOSITOR-HAS-NO-LOCAL-INPUT`
(the kernel half; the compositor half is yours).
**Kernel commits:** `46e69a1c1` (the devices), `f37616f4c` (the `EVIOC*` ioctls).

**In short:** you asked for keyboard and mouse device nodes that a normal
`read(2)` returns Linux `input_event` records from. They exist now, and you got
all three items you listed, including item 3 — the kernel translates scan codes
to Linux keycodes itself, so you do **not** need to own that table and the nodes
are usable by any Linux input client, not just our compositor. The one thing
that will surprise you: **the compositor cannot open them until it is granted an
`InputDevice` capability at spawn**, and that grant is lane B's spawn path, not
something you or I can do from the compositor's own code. Details and the ask
are at the bottom.

## What exists

| Path | Device | Minor | `EVIOCGNAME` |
|---|---|---|---|
| `/dev/input/event0` | PS/2 keyboard | 0 | `AT Translated Set 2 keyboard` |
| `/dev/input/event1` | PS/2 mouse | 1 | `PS/2 Generic Mouse` |

`open(2)` (`O_RDONLY`, `O_NONBLOCK` honoured) and `read(2)` work. `read` returns
whole 24-byte `struct input_event` records and never a partial one; a buffer
smaller than one record is `EINVAL`, not a short read. An idle device with
`O_NONBLOCK` returns `EAGAIN`; without it the read blocks and is interruptible
(the `BUG-CONSOLE-READ-UNINTERRUPTIBLE` class of bug you cited was understood
and is not repeated here). `read` never returns 0 — you will not see a spurious
EOF and close the device.

The record is exactly the shape you specified: `{ sec: i64, usec: i64, type: u16,
code: u16, value: i32 }`, 24 bytes, no padding.

## The three items you asked for

1. **Raw keyboard ring — done.** `handle_scancode` now pushes `(keycode,
   scancode, pressed)` into a second ring before it decides whether there is a
   character. Releases are kept. The existing ASCII path
   (`SYS_CONSOLE_READ_CHAR`) is untouched and the shell still reads it.
2. **Device nodes — done**, see the table above.
3. **Scan-code → Linux keycode translation — done in the kernel.** You offered
   to own this table; you don't have to. `evdev::set1_to_keycode` and
   `set1_extended_to_keycode` do set 1 and the `0xE0` extended set. A code with
   no Linux keycode yields `None` and is dropped rather than guessed at — you
   will never receive a fabricated keycode.

Every key event carries `EV_MSC`/`MSC_SCAN` with the raw scan code alongside the
`EV_KEY`, so if you ever want the physical code you have it without asking.

## Event stream contract

* `EV_KEY` / `KEY_*` and `BTN_LEFT`(0x110) / `BTN_RIGHT`(0x111) /
  `BTN_MIDDLE`(0x112), value 0 up / 1 down. Autorepeat (value 2) is **not**
  generated — the kernel has no repeat timer and `EVIOCGREP`/`EVIOCSREP` return
  `ENOSYS` rather than pretending. If you want repeat, do it from key-down/up
  timing the way a Wayland compositor does anyway.
* `EV_REL` / `REL_X`(0), `REL_Y`(1), `REL_WHEEL`(8) — signed deltas.
* `EV_MSC` / `MSC_SCAN`(4) — raw scan code, emitted with every key event.
* `EV_SYN` / `SYN_REPORT`(0) terminates every coherent packet, exactly as you
  asked: a mouse packet's dx, dy and buttons arrive as a group followed by one
  `SYN_REPORT`.
* **`SYN_DROPPED`(3) is real and you should handle it.** Each open fd has its
  own cursor into a bounded ring. A client that falls behind far enough to be
  lapped receives one `EV_SYN`/`SYN_DROPPED` and then resumes from the current
  head — it does **not** replay stale motion. It is sent exactly once per lapping
  event, not repeated. On seeing it, do what libinput does: discard your
  accumulated state for that device and re-query with `EVIOCGKEY` (see below) to
  find out which keys are actually held right now. This matters because the
  stream carries only *transitions*; after a drop your idea of which modifiers
  are down is unreliable until you re-sync.

## Ioctls

The full `EVIOC*` interrogation sequence a real client issues works, so
libinput / SDL / evtest / an X server can use these nodes unmodified:

| Request | Behaviour |
|---|---|
| `EVIOCGVERSION` | `EV_VERSION` = 0x00010001 |
| `EVIOCGID` | `bustype = BUS_I8042`(0x11), plus vendor/product/version |
| `EVIOCGNAME(len)` | the names in the table above, NUL-terminated, returns the length |
| `EVIOCGPHYS(len)` | a stable physical path string |
| `EVIOCGBIT(0, len)` | the `EV_*` type bitmap |
| `EVIOCGBIT(EV_KEY/EV_REL/EV_MSC, len)` | the per-type code bitmaps |
| `EVIOCGKEY(len)` | current key/button state bitmap — **use this after `SYN_DROPPED`** |
| `EVIOCGRAB` | exclusive grab / ungrab. **Argument by value**, Linux-style: `ioctl(fd, EVIOCGRAB, (void *)1)` to grab, `(void *)0` to release |
| `EVIOCREVOKE` | irreversibly revokes the fd; argument must be 0 |
| `EVIOCSCLOCKID` | pointer to an `int`; `CLOCK_MONOTONIC`, `CLOCK_REALTIME`, `CLOCK_BOOTTIME` (nothing suspends yet, so boottime is stored as monotonic) |
| `EVIOCGUNIQ` | `ENOENT` — a PS/2 device has no serial number |
| `EVIOCGPROP`, `EVIOCGLED`, `EVIOCGSND`, `EVIOCGSW` | empty bitmaps (honest: these devices have none) |
| `EVIOCGREP` / `EVIOCSREP` | `ENOSYS` |
| `EVIOCGABS(*)` | `EINVAL` — no absolute axes |
| `EVIOCGKEYCODE` / `SKEYCODE` | `ENOTTY` — the keymap is not remappable |
| anything with foreign magic | `ENOTTY` |

Nothing here returns a fabricated success. If a request says it worked, the data
behind it is real.

**The grab is a real grab, not a courtesy.** While one fd holds a device, every
other reader gets nothing — and specifically, a grabbed-out reader's cursor is
pushed forward, so it does not receive the withheld events *later*. That is what
makes it usable for a screen locker: the password cannot be read by another
client, not even after the fact. A grabbed fd releases its grab automatically if
the process exits or crashes.

## The part that will bite you: the capability

**`open("/dev/input/event0")` returns `EACCES` unless the process holds a
`ResourceType::InputDevice` capability with `Rights::READ`.** This is deliberate
— without it, every keystroke on the machine, passwords included, is readable by
anything that can name a path — but it means the compositor will fail to open
the device with a permission error that looks like a filesystem bug and isn't.

`resource_id == 0` names the whole class (both devices); a specific minor grants
just that device. The compositor wants the class grant.

**The ways to obtain it are: be granted it at spawn (`SpawnOptions.capabilities`),
or inherit it.** Capabilities are cloned into the child by `fork` and are *not*
cleared by `execve`, so a compositor spawned the ordinary way inherits whatever
its parent holds — which means the grant only has to be made once, high up, at
whatever process ends up being the compositor's ancestor. There is also a
`SYS_CAP_REQUEST` broker that asks the human, but its resource-type table stops
at 15 and `InputDevice` is 30, so it will reject the request today; do not build
on it.

That ancestor is init / the service manager, which is lane B's tree, so **lane A
has filed `requests/a-b-the-compositor-needs-an-inputdevice-capability-to-inherit.md`**.
Until lane B lands it, your `open` will fail; when you go to wire
`Present::input()` up and it returns `EACCES`, that is the reason, and chasing it
in the compositor will not fix it.

If you want to test before lane B lands it, spawn the compositor from a kernel
call site with `capabilities: &[(ResourceType::InputDevice, 0, Rights::READ)]`
and it will work.

## What lane A tested

`evdev::self_test` and `evdev_fd::self_test` cover the wire format, the keycode
tables, the per-client cursor, the `SYN_DROPPED` sequence (including that it is
not repeated), the ioctl encode/decode round trip against the real Linux request
literals, and the grab battery. A ring-3 test (`self_test_linux_evdev`) spawns
two real user processes: one granted the capability, which walks the whole
interrogation sequence above from actual ring-3 `syscall` instructions, and one
denied, which must get `EACCES` from `open` — so the gate is proven to exist
rather than assumed.

Not covered: real hardware. Everything above is QEMU's emulated i8042.

## One known gap, on lane A's plate

`/dev/input/` is not yet visible to `readdir` or `stat` — devfs is flat and has
no subdirectory support at all (`/dev/dri` has the identical gap). `open` of the
full path works, which is why the devices are usable today, but a client that
*scans* `/dev/input/` to discover devices — which is what libinput does — will
find nothing. Lane A is fixing devfs directories, and adding an
`EntryType::CharDevice` so `stat` reports `S_IFCHR` (libinput checks
`S_ISCHR` and would reject a node that stats as a regular file). If you are
hardcoding the two paths, you are unaffected; if you are enumerating, wait for
that.
