### A-PS2-KEY-HELD-SINCE-POWER-ON-IS-NOT-SEEN -- 2026-10-02 -- OPEN, a hardware limit (lane A)

**Status:** OPEN (lane A) -- a limitation, with no fix known to be safe.

**In short:** the operator's "hold Shift at start-up to choose an account"
(design-decisions 1427) needs the kernel to know that Shift has been held
since the machine was switched on. A USB keyboard is asked directly
(`xhci::hid_get_keyboard_report`, GET_REPORT). A PS/2 keyboard -- including
most laptops' built-in keyboards -- cannot be asked: PS/2 has no command that
reports which keys are down. The kernel learns of a held key only if the
keyboard keeps repeating it, and the kernel's own "start scanning" command
(`0xF4`, in `keyboard::init`) stops the repeat on a keyboard that follows
IBM's specification ("clears the last typematic key"). On such a keyboard a
Shift held from power-on reads as up until it is let go and pressed again.

**Where:** `kernel/src/keyboard.rs`, `init` (the `KB_CMD_ENABLE_SCAN` write
after `clear_key_state`).

**Options, none clearly safe:**
- Skip `0xF4` and rely on the firmware having left scanning on. Keeps the
  repeat, but a firmware that left scanning off leaves the keyboard dead, and
  nothing can tell the two apart.
- Reset the keyboard (`0xFF`) and rely on it reporting keys found down after
  its self-test. Device-dependent, and the self-test takes up to a second.
- Listen for a repeat for ~100 ms before sending `0xF4`. Works only while the
  firmware's repeat is running, and costs that time on every boot.

**Trigger:** a real PS/2 or laptop keyboard to try the options on. Linux
sends `0xF4` too and has the same blind spot.
