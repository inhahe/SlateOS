## §456 — Scancodes become key names in the compositor, and the raw code rides along anyway

**Date:** 2026-08-17
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** When you press a key, the hardware reports *which switch closed* —
a number like 30 — not *which letter it means*. Something has to turn 30 into
"A", and that depends on your keyboard layout (30 is "A" on a US layout and "Q"
on a French one). The question was whether the display server does that
translation once for the whole system, or whether every application does it for
itself. Decision: the display server does it, and *also* forwards the raw
number, so the handful of programs that genuinely want physical key positions
can still have them.

**Context.** `guiremote` gained an input direction (`gui/remote/src/input.rs`)
so the compositor can deliver key and mouse events to clients — see
`known-issues.md` → `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` for why there was
none. The two ends spoke different languages about keys, and the wire format
had to pick one:

- The compositor's internal `EventNotification::KeyEvent`
  (`gui/compositor/src/main.rs:3129`) carries `scancode: u32` plus
  `character: Option<char>` — hardware-level.
- `guitk::event::KeyEvent` (`gui/toolkit/src/event.rs:69`) carries a semantic
  `Key` enum — `Key::Left`, `Key::F5`, `Key::Semicolon` — which is what widget
  code is written against.

No scancode→`Key` mapping existed anywhere in the tree; grepped, `scancode`
appears only inside the compositor. So the translation had to be written, and
where it lives was open.

**Options.**

1. **Translate in the compositor; send `Key`.**
   *What changes:* an app receives `Key::Semicolon`; changing the system
   keyboard layout in Settings immediately affects every running app with no
   cooperation from any of them.
2. **Send raw scancodes; each client translates.**
   *What changes:* an app receives `30`; every app must load a keymap, and one
   that does not, or loads a stale one, silently disagrees with the rest of the
   desktop about what the user typed.
3. **Translate in the compositor, and send the scancode too.**
   *What changes:* the same as (1) for every ordinary app, plus a `scancode`
   field that a game or a remapping utility can read to get physical key
   positions.

**Decision: (3).**

**Reasoning.**

- (2)'s failure mode is the disqualifying one. A keymap is *system* state — it
  belongs to the user, not the application — and distributing it to 138 client
  crates means 138 chances to be out of date. The version skew is invisible:
  nothing looks broken, the wrong letter simply appears. Wayland can afford
  client-side keymaps because it ships the keymap over the same protocol and
  every client uses one library (xkbcommon) to read it; we have neither, so (2)
  would mean writing that whole apparatus to be no better off.
- Against (1) — and the reason (3) exists — is that semantic keys genuinely
  lose information some programs need. A game binding "the key left of S" wants
  the physical position regardless of layout; a remapper is *about* physical
  positions. Under (1) those programs are simply not writable.
- (3) costs five bytes per key event (a presence byte and a `u32`) and settles
  the argument by not having it: the central keymap governs, and nothing is
  foreclosed. Five bytes on an event that occurs at human typing speed is not a
  cost worth reasoning about.
- The scancode is on the *wire* type (`InputEvent::scancode`), not inside
  `guitk::event::KeyEvent`. Putting it in `KeyEvent` would have obliged all
  ~539 places in this tree that construct one to supply a value, nearly all of
  them tests for which a scancode is meaningless. A widget reacting to a key
  wants to know it was `Key::Left`; it has no use for which switch closed.

**Consequences and what is still owed.** The compositor does not yet *have* the
mapping table this decision assigns to it, nor modifier-state tracking (its
`handle_key` passes `pressed` through without accumulating shift/ctrl/alt/super
into a `Modifiers`). Both are now compositor work, and both are prerequisites
for the first real client. The wire format is version 1 (`INPUT_VERSION`) and a
later change to any of this is a version bump, not a silent reinterpretation.

**Reversibility.** Moderate. Moving to (2) later would mean adding a keymap
distribution message and is a protocol version bump; the `scancode` field means
the data a client-side translation needs is already arriving, so the change
would be additive rather than a break.
