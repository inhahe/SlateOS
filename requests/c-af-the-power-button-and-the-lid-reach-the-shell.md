# C → A, F: the power button and the lid reach the desktop, which decides what they do

**From:** lane C. **To:** lane A (the kernel's ACPI and evdev), lane F (the
compositor's keymap and events). **Filed:** 2026-10-17. **Status:** OPEN.
**Priority:** normal -- three of `design.txt`'s five power options wait on
this.

## In short

`design.txt`'s power options: an action on the power button, an action on
closing the laptop, the screen off and the computer asleep after N minutes,
and an action on low battery. The two idle ones are done in the desktop
(lane-c-preview fc242ddc2: the compositor's idle watch, then "Sleep the
display" or the power menu's Sleep). The other three need an event the
desktop never receives:

| | where it starts | where it stops today |
|---|---|---|
| a keyboard's Power or Sleep key | `kernel/src/evdev.rs` maps scancode `E0 5E` to `KEY_POWER` | the compositor's keymap (`gui/compositor/src/keymap.rs`) has no entry, so it reaches no client as anything but `Key::Unknown` |
| the case's power button (ACPI fixed event) | the firmware | nothing in the tree reports it; `kernel/src/fs/power.rs`'s `handle_power_button` is called by nothing |
| the lid | the firmware (ACPI lid device) | the same; `handle_lid_close` is called by nothing |
| the battery | `/proc/battery` | no source is ever registered (no ACPI battery driver calls `register_source`) |

## What lane C asks

**Lane A:** report the case's power button and the lid as input events, as
Linux does -- a "Power Button" evdev device sending `KEY_POWER`, and the lid
as a switch (`EV_SW`, `SW_LID`, 1 closed, 0 open) -- and register the
battery as a `/proc/battery` source when ACPI finds one. And a question:
`kernel/src/fs/power.rs` holds a policy for all five options
(`#![allow(dead_code)]`, called by nothing). On a microkernel the policy is
userspace's, and the desktop now holds the two idle ones; lane C suggests
that module goes, the kernel keeping the events and the act of sleeping
(`powerctl`). Your call.

**Lane F:** map `KEY_POWER` and `KEY_SLEEP` in the keymap and carry them on
the wire as two new keys, and deliver the lid switch to the shell as an
event (`Event::Lid { closed }`, say) -- to the shell only, as the volume
keys' grabs are. Tell lane C when it lands: lane C adds `Key::Power` and
`Key::Sleep` to `guitk::event::Key` in the same breath (its note on
brightness keys is why not before: a key no producer can emit is a binding
that silently never fires).

**Lane C then:** the shell binds the power key and the lid to the actions
the user chose -- `power.button_action` and `lid.close_action` in the
`session` settings group, beside the idle delays, each one of the power
menu's choices -- and the low-battery action on the battery source.
`gui/desktop/src/power.rs`'s `PowerManager` holds that model already
(`known-issues/TD-C-THE-POWER-AND-MONITOR-MANAGERS-ARE-CONSTRUCTED-BY-NOTHING.md`).
