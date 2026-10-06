## `TD-C-BRIGHTNESS-KEYS-ARE-NOT-KEYS` (lane C, 2026-08-26)

**Status:** OPEN for the second half only (a source of brightness events).
**2026-10-06: the first half exists, and the actions use it.** The kernel
reports each display's brightness (`/proc/brightness`) and sets it
(`SYS_BRIGHTNESS_SET`, for a process holding `SET_BRIGHTNESS`), and
`HotkeyAction::BrightnessUp`/`BrightnessDown` now step the screen's
brightness through it (`gui/desktop/src/backlight.rs`,
`DesktopShell::step_brightness`), the overlay showing the level -- or, while
the desktop is not given the right, saying "Can't be changed yet"
(`TD-C-THE-DESKTOP-CANNOT-SET-THE-BRIGHTNESS-IT-SHOWS`). So, as this entry
foresaw, the actions are useful bound to a chord of the user's choosing
(Super+F5, say). What stays open is part 2 below: a laptop's own pair still
reaches nothing, because it sends no key; its ACPI notify events are lane A's
to deliver as system events.

**In short:** the desktop has actions named "brightness up" and "brightness
down", but nothing in this system can ever trigger them, and nothing in this
system can change a screen's brightness even if they were triggered. A laptop's
brightness pair is not a key: pressing Fn+F5 does not send a scancode (the
number a keyboard sends when a key goes down) at all. The firmware handles it
itself and talks to the panel over ACPI (the firmware's power-management
interface) or a vendor-specific WMI channel. So there is no key event for a
keyboard table to translate, and the two actions sit in the hotkey table
unbound.

**Where it lives.**

| File | What is there |
|---|---|
| `gui/desktop/src/hotkeys.rs` | `HotkeyAction::BrightnessUp` / `BrightnessDown` exist and are parseable from a config file, but `install_defaults` gives them no binding, and `parse_key_name` *rejects* `"brightnessup"` / `"brightnessdown"` |
| `gui/toolkit/src/event.rs` | deliberately has no `Key::BrightnessUp` / `Key::BrightnessDown` variant |
| `gui/compositor/src/keymap.rs` | `key_for_scancode` has arms for the seven media keys and deliberately none for brightness |

**Why it was left this way rather than "fixed" by adding the variants.** A
`Key` variant no producer can ever emit is exactly the bug this change was
undoing. Before it, the shell bound the volume keys to `Key::Unknown(0xAF)` and
its neighbours — Windows virtual key codes, which this system never emits — so
the bindings matched nothing for the whole life of the module, and a binding
that matches nothing looks exactly like a binding nobody has pressed. Adding
`Key::BrightnessUp` would recreate that: a name that parses, a binding that
registers, and silence when pressed. Rejecting the name in `parse_key_name`
means a config file that asks for it fails loudly instead, which is the honest
answer.

**What the proper fix is** — two independent halves, in this order:

1. **A backlight service.** Something has to be able to *set* brightness before
   an event to change it means anything. On real hardware that is an ACPI
   `_BCM` call or a vendor WMI method; the natural home is a userspace service
   exposing a "set backlight to N%" request, with the compositor as its client
   (it already owns the display). Until this exists, both halves of the feature
   are decoration.
2. **A source of brightness events.** Firmware that handles the keys itself
   often *also* reports them as ACPI notify events on the video device rather
   than as keystrokes, so the source is the ACPI subsystem, not the keyboard
   driver — it must not be plumbed through `key_for_scancode`. Some external
   USB keyboards do send HID consumer-page brightness usages, which would come
   in through the HID path instead. Whichever arrives, it should reach the shell
   as a *system event*, not as a `Key`.

Once (1) exists, `HotkeyAction::BrightnessUp` becomes bindable to a chord of
the user's choosing (Super+F5, say) and is immediately useful without (2).

**What happens if nothing is done.** Nothing gets worse. The actions are inert,
the config parser rejects the key names loudly, and no user-visible feature
silently misbehaves. This is a missing feature with an honest failure mode, not
a bug.
