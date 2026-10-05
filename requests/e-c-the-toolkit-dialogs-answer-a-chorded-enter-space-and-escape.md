# E → C: the toolkit's dialogs answer a chorded Enter, Space and Escape

**From:** lane E · **To:** lane C (`gui/toolkit/src/modal.rs`) · **Filed:** 2026-09-29
**Status:** open. Nothing in lane E is blocked; every program that shows one
of these dialogs inherits the fault until it is fixed here.

## In short

A key pressed with Alt or the Windows key is a command for the window or the
desktop, not for whatever has the keyboard. The toolkit's dialogs do not look
at what is held, so Alt+Space -- on Windows, the window menu -- presses the
dialog's focused button just as Space does. In the disk imager that button can
be "Write", which erases a drive.

Measured 2026-09-29 against `origin/main` with a throwaway test on
`AlertDialog::destructive("Confirm Write", …, "Write")`, the confirmation
`apps/diskimager` shows before it overwrites a drive:

| Pressed | Focus left on Cancel (the default) | After Tab, focus on Write |
|---|---|---|
| Alt+Space | `Cancel` | **`Ok` -- the drive is written** |
| Alt+Enter | `Cancel` | **`Ok`** |
| Windows+Space, Windows+Enter | `Cancel` | **`Ok`** |
| Alt+Escape, Windows+Escape | `Dismissed` | `Dismissed` |

## Where

`gui/toolkit/src/modal.rs`: none of these asks `event.modifiers` beyond Shift.

- `ModalOverlay::handle_key` (Escape)
- `AlertDialog::handle_key` (Tab, Enter, Space)
- `InputDialog::handle_key` (Escape, Enter, Space, Tab)
- `ProgressDialog` (Escape cancels)
- `NonModalDialog::handle_key`

## What lane E does in the programs meanwhile

The rule in `apps/textline`, applied program by program in lane E's keys pass
(`known-issues.md` "[E] A key held with Alt or the Windows key works a
program's bare-key binding"):

- `is_plain(modifiers)`: nothing but Shift held. The test for a key bound as
  itself (Enter, Space, Escape, a letter shortcut).
- `is_ctrl_chord(modifiers)`: Ctrl without Alt or the Windows key. The test for
  a Ctrl shortcut, since AltGr arrives as Ctrl+Alt and types.
- `is_command(modifiers)` / `types_into_field(key)`: whether a key types into
  a field.

For the dialogs, the fix is `is_plain` on each of their keys. Shift+Tab still
walks back.

## A suggestion, structural

These questions belong on `guitk::event::Modifiers` / `KeyEvent`, not in a
lane E crate. The toolkit's own widgets need them: these dialogs,
`TextInput::edit_key` (`e-cf-a-toolkit-field-types-the-letter-of-a-shortcut-it-does-not-know.md`),
and the list and tree keys. With one definition the programs and the toolkit
cannot drift apart. If you add them, lane E moves `textline`'s callers onto
them and drops its copies.
