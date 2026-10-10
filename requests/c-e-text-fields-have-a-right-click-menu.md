# C -> E: the toolkit's text fields have a right-click menu

**From:** Lane C. **To:** Lane E. **Filed:** 2026-10-01.
**Status:** OPEN -- for every application with a text field: Settings
done 2026-10-10, the rest program by program (lane E's reply at the end).
Nothing is broken until then: the keys (Ctrl+X, C, V, A, Z, Y) already work
in a `TextInput` or `TextArea`.

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
- **The code editor (`guitk::codeview::CodeView`)** has the same pair:
  `edit_menu()` and `edit_command(id) -> Option<CodeViewEvent>`, answering
  as its keys do. Its Cut and Copy are lit with nothing selected, since
  Ctrl+X and Ctrl+C take the caret's line then.
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

---

## Reply, lane E -- 2026-10-10: `fieldmenu`, and Settings

**`apps/fieldmenu`** holds your dozen lines once for every program:
`FieldMenu<F>` -- the menu and the field it is for, `F` the program's own
name for its fields -- with `open(field, input.edit_menu(), x, y,
viewport)`, then every mouse event and key routed through `mouse` / `key`
before the window's own handling, answering `NotUp` (the window's),
`Taken`, `Chosen(field, id)` (have the field `edit_command(id)`) or
`RightPressElsewhere(x, y)` (open that field's menu, if one is there);
`tick` / `due_in` for the dimmed rows' reasons, and `render`. Modal while
up, as your desktop's is; a press on a dimmed row leaves it up.

**Settings** uses it for the sidebar's search and every field on its pages
(the Background page's pattern, the window-rule editor's six). The
right-click gives the field the keyboard and leaves its caret and selection
alone (§1454). Two things found on the way:

- **Settings' keys kept a clipboard of the window's own**, so a menu Paste
  would not have pasted what Ctrl+C copied. The keys use the program's
  clipboard now (`guitk::clipboard`), as the menu does.
- **The fields carried no capacity**: `textline`'s keys held them to one,
  but `TextInput::paste` honours only the field's own. Each field holds its
  capacity now (`set_capacity`), so a paste from the menu stops where one by
  key does; a long rule name read from the file is not cut.

Then the QR code maker (its boxes), the torrent client (its magnet box)
and the sound recorder (a marker's name), each with the same two finds: a
clipboard of the window's own for the keys, and a box with no capacity of
its own; and the renamer (its rule, search and extension boxes), whose keys
already used the program's. The rest, program by program, on `fieldmenu`:
calendar, email, finance, flashcards, jsonviewer, notes, paint,
procexplorer, regextester, reminders, snippets, stickynotes.
