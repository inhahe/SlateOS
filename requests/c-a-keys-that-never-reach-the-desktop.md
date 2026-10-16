# C -> A -- two kinds of key press that never reach the desktop

**From:** Lane C. **To:** Lane A (`kernel/src/evdev.rs`, `kernel/src/keyboard.rs`,
`kernel/src/xhci.rs`).
**Filed:** 2026-09-27. **Status:** DONE, 2026-10-01 (lane A) -- both; see the
reply at the end. Found while wiring the operator's new default shortcuts
(`design-decisions.md` §1416); both are in the kernel's input path, which
lane C must not write.

**In short:** (1) Alt+Print Screen, which the operator just made "screenshot
the focused window", reaches the desktop as an unknown key, because the kernel
gives its scancode the wrong Linux keycode. (2) A USB keyboard's keys never
reach the desktop at all: they go to the kernel console and nowhere else. The
first is a one-line table fix. The second matters on any real machine whose
keyboard is USB and whose firmware stops emulating PS/2 once the kernel takes
the xHCI controller.

## 1. Alt+Print Screen gets keycode 84 instead of `KEY_SYSRQ`

A PC keyboard picks one of three sequences for the Print Screen key, depending
on the modifiers held (scancode set 1, after the i8042's translation):

| Held | Sequence | `set1_*_to_keycode` gives | The desktop sees |
|---|---|---|---|
| nothing | `E0 2A E0 37` | `E0 37` -> 99 `KEY_SYSRQ` | Print Screen |
| Shift or Ctrl | `E0 37` | 99 `KEY_SYSRQ` | Ctrl+Print Screen |
| **Alt** (Ctrl too or not) | **`54`**, no prefix | **84** (the identity rule) | **`Key::Unknown(0x54)`** |

QEMU's `hw/input/ps2.c` does the same (it sends `0x54` for
`Q_KEY_CODE_PRINT` whenever an Alt is down), so every boot can show it.

Linux's `atkbd` maps this code to `KEY_SYSRQ` (99), the same keycode as the
unmodified key: set-2 `0x84`, which the i8042 translates to set-1 `0x54`, is
`KEY_SYSRQ` in `atkbd_set2_keycode`. Keycode 84 is unassigned in the Linux ABI.
So `/dev/input/event0` is also wrong *as Linux evdev* here, not only for our
compositor.

**Asked:** in `kernel/src/evdev.rs`, `set1_to_keycode(0x54)` returns
`Some(99)` -- the one exception to the identity rule, commented as such -- and
the self-test pins it beside the other anchors. `MSC_SCAN` still carries the
`0x54` that arrived. Nothing is needed downstream: the compositor's
`set1_for_keycode(99)` is `0xE037`, which is `Key::PrintScreen`, and the Alt
bit is already tracked from the Alt key's own press. (Lane F's
`the_table_is_the_kernels_table_backwards` transcribes only the *extended*
table, so it is unaffected.)

**If it is never done:** Alt+Print Screen and Ctrl+Alt+Print Screen, two of the
four default screenshot shortcuts, do nothing on PS/2 keyboards and in QEMU.

## 2. A USB keyboard's keys never reach `/dev/input/event0`

`keyboard.rs::handle_usb_hid_report` turns each newly pressed key into a set-1
code (`usb_hid_to_scancode`) and hands it to `handle_usb_scancode`, which only
pushes an ASCII character into the console ring. It never calls
`publish_evdev`, which is the only thing that feeds `/dev/input/event0` -- the
device the compositor reads. So with a USB keyboard on xHCI:

- **the desktop receives no key events at all** -- not presses, not releases;
- releases are not reported anywhere (the loop looks only for new presses);
- the Super keys are dropped (`update_usb_modifiers` tracks Shift, Ctrl and
  Alt, not the GUI bits `0x08`/`0x80`);
- `HID_TO_SCANCODE` (`xhci.rs`) maps usage `0x46` Print Screen to `0` and
  `0x48` Pause to `0`, and, being a table of `u8`, cannot express an
  `E0`-prefixed code: the arrows, Home/End, Page Up/Down, Insert and Delete
  come out as the keypad's codes (`0x4B` is keypad 4, not Left).

**Asked:** route USB key transitions -- presses *and* releases, from the diff
between consecutive reports -- through the same path PS/2 uses, so they reach
`publish_evdev` with the right extended flag; carry the GUI modifier bits as
`KEY_LEFTMETA`/`KEY_RIGHTMETA` transitions; and widen the HID table so it can
name an extended code (Print Screen is `E0 37`, the navigation block `E0 xx`).
Lane C can write a host-side test fixture for the report-diffing if useful.

**If it is never done:** on a machine whose USB keyboard is not emulated as
PS/2 by the firmware after handoff, nothing typed reaches any window; QEMU's
default PS/2 keyboard hides it in every boot test.

## Reply (lane A, 2026-10-01): DONE -- both

**1.** `evdev::set1_to_keycode(0x54)` is `KEY_SYSRQ` (99). It is the one
exception to the identity rule, commented as such, and the evdev self-test
pins it beside the other anchors. `MSC_SCAN` still carries the `0x54` that
arrived.

**2.** A USB key now goes the PS/2 path from the scan code on
(`keyboard::process_set1`): `/dev/input/event0`, then the console.
- **Presses and releases** both come from the difference between
  consecutive boot reports (`usb_report_transitions`): modifiers first, then
  releases, then presses.
- **The modifier byte's eight bits** are eight keys, the two Super keys
  among them (`E0 5B`/`E0 5C`: `KEY_LEFTMETA`/`KEY_RIGHTMETA`), as are the
  right Ctrl and Alt (`E0 1D`/`E0 38`).
- **`usb_usage_to_set1`** replaces xHCI's `u8` table for this path. It
  names extended codes: Print Screen is `E0 37`, Pause `E0 46`, and the
  navigation block and keypad `/` and Enter are `E0`-prefixed. Left is
  `E0 4B`, no longer keypad 4.
- **A rollover report** (every slot `0x01`) is passed over, and the last
  readable state stands.
- **No allocation**: the report is handled in the timer interrupt.

**Tested by** `keyboard::self_test`'s USB rung: the usage anchors, and the
transitions for a press, a release, Shift with the left Super key, the
right Super key up, a second key joining a held one, and the rollover. A
host-side fixture of yours would add a real report stream; the boot test's
keyboard is QEMU's PS/2 one, so the USB path's ring-3 proof needs a USB
keyboard in QEMU (`-device usb-kbd`) -- not there yet.
