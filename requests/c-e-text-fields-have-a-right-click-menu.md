# C -> E: the toolkit's text fields have a right-click menu

**From:** Lane C. **To:** Lane E. **Filed:** 2026-10-01.
**Status:** OPEN -- for every application with a text field. Nothing is
broken until then: the keys (Ctrl+X, C, V, A, Z, Y) already work in a
`TextInput` or `TextArea`.

**In short:** A right-click in a text box should offer Cut, Copy, Paste,
Delete and Select all (and Undo and Redo in a multi-line box), each greyed
when it would do nothing -- as the desktop's own boxes now do (the Run box,
the start menu's search, a note, an icon being renamed). The toolkit gives
you the menu's rows and does what the chosen row says; your window puts the
menu up where the right-click landed, as it does any other menu. About a
dozen lines per window.

## How an application uses it

```rust
use guitk::menu::{ContextMenu, MenuAction};

// A right press over the field: the menu, at the press.
if let MouseEventKind::Press(MouseButton::Right) = event.kind
    && field_rect.contains(event.x, event.y)
{
    let mut menu = ContextMenu::new(field.edit_menu());
    menu.show(event.x, event.y, viewport);
    self.field_menu = Some(menu);
}

// A row chosen -- by click (`menu.handle_click`) or key (`menu.handle_key`
// -> `MenuAction::Selected(id)`): the field does it, and says what it did,
// as `edit_key` does for a key.
match field.edit_command(id) {
    KeyEdit::Changed => self.text_changed(),   // whatever typing does
    KeyEdit::Handled | KeyEdit::Unhandled => {}
}
```

- **`TextArea::edit_command(id, &metrics)`** takes the box's metrics, as
  its keys do, so the caret stays in view after a paste.
- **Your own rows beside them.** The rows' ids are far up the id space
  (`guitk::editmenu::EditCommand::id`), so a "Paste and go" or "Insert
  date" of yours can follow them in the same menu with your own small ids;
  `edit_command` answers `Unhandled` for an id that is not one of its rows,
  and `EditCommand::from_id` says which is which.
- **A read-only field** (a log, a property sheet's value): build the rows
  yourself with `guitk::editmenu::rows(EditState { selected, has_text,
  ..EditState::default() })` -- Copy and Select all lit, the rest greyed.
- **Leave the caret alone on the right-click** when the field already has
  the keyboard: Cut and Copy act on the selection the user made
  (`design-decisions.md` §1454).
- Paste is greyed while the program's clipboard is empty. The clipboard is
  each program's own until open question C-Q29 is answered; nothing in your
  code changes when it crosses programs.

## Where

Every window with a `TextInput` or a `TextArea`. On 2026-10-01 these
directories under `apps/` use one: calendar, email, finance, flashcards,
jsonviewer, notes, paint, procexplorer, qrcode, regextester, reminders,
renamer, settings, snippets, soundrecorder, stickynotes, textarea,
textline, torrent. The desktop's wiring, as a worked example:
`gui/desktop/src/lib.rs`, `open_field_menu`, `click_field_menu`,
`activate_field_menu_item`, and the routing for `field_menu` at the top of
`handle_mouse_inner` and in `handle_hotkey_inner`; tests in
`gui/desktop/src/field_menu_tests.rs`.
