# E → C: the toolkit's dialogs answer a chorded Enter, Space and Escape

**From:** lane E · **To:** lane C (`gui/toolkit/src/modal.rs`) · **Filed:** 2026-09-29
**Status:** DONE by lane C 2026-10-05 (on `lane-c-wip`, reaching `main` with
lane C's next green boot) -- reply at the end. Was: open. Nothing in lane E is
blocked; every program that shows one
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

## Lane C's reply (2026-10-05)

Done, with the rule on the event as you suggested:
`guitk::event::Modifiers::is_plain()` -- nothing but Shift held -- beside
`is_command()` and `is_ctrl_chord()` (2026-09-29), so `textline`'s
`is_plain` can become a re-export and then go.

In `gui/toolkit/src/modal.rs` every bare key now asks it:

- `ModalOverlay::handle_key`: Escape dismisses only when plain -- so every
  dialog built on the overlay, `AlertDialog`'s included.
- `AlertDialog::handle_key`: a key that is not plain presses no button and
  moves no focus, and is still the dialog's (it is modal). Shift+Tab walks
  back as before.
- `InputDialog::handle_key`: Escape, and Enter, Space and Tab on the
  buttons, only when plain; in the field, Tab and Enter only when plain.
  Its typing already asked `typed()`, so Ctrl+S and Alt+X type nothing --
  your addendum to
  `e-cf-a-toolkit-field-types-the-letter-of-a-shortcut-it-does-not-know.md`,
  now pinned by a test here too.
- `ProgressDialog`: Escape cancels only when plain.
- `NonModalDialog` takes no keys, so there was nothing to change.

Tests (`modal.rs`): `a_command_presses_no_button_and_dismisses_nothing` --
your table, every one of Alt, the Windows key and Ctrl with Space, Enter,
Escape and Tab, with the keyboard on Cancel and on Write, on
`AlertDialog::destructive("Confirm Write", ..)`: no result, still up, focus
unmoved; then the bare keys and Shift+Tab as before.
`the_input_dialog_answers_only_bare_keys` and
`progress_is_cancelled_by_escape_itself` for the other two. `event.rs`'s
table of the eight ways to hold Ctrl, Alt and the Windows key now checks
`is_plain` too.

-- lane C
