# C → E — A multi-line text field for the apps that edit text

**From:** Lane C (`gui/toolkit`). **To:** Lane E (`apps/notes`, `apps/stickynotes`, `apps/email`).
**Filed:** 2026-09-25. **Status:** DONE (lane E, 2026-09-28) for the three apps
named: `apps/notes` (the body read and written through one field; a7d884c06),
`apps/stickynotes` (the text with its list markers, a list carried on by Enter;
1fc6ddb33) and `apps/email`'s compose body (wrapped, undo, the window's one
clipboard). Each takes clicks, drags, double clicks and the wheel, and the
user's caret width. Lane E's interim `apps/textarea` still serves regextester,
jsonviewer and snippets: regextester moves next; the two code editors need a
font family in `textarea::Metrics` (`requests/e-c-the-multi-line-field-needs-a-font-family.md`).

**In short:** the toolkit has a multi-line text field now,
`guitk::textarea::TextArea`, with its drawing in `textarea::draw`. The apps
that edit more than one line each have their own, or none: `apps/notes`
appends what is typed to the end of the body (`body.push_str(&key.text)`,
`body.push('\n')`), so a note cannot be corrected except by deleting back to
the mistake; `apps/stickynotes` carries its own caret and editing
(`handle_key`, about line 3097); `apps/email`'s compose appends typed text.

## What the field does

A caret and selection over wrapped lines, right-to-left lines included; Left
and Right by the screen, Up and Down holding their column, Home and End per
line, Ctrl+Home and Ctrl+End, Page Up and Page Down; click, drag and double
click; a view the wheel scrolls (`scroll_by`); undo (Ctrl+Z) and redo (Ctrl+Y,
Ctrl+Shift+Z) a word at a time; cut, copy and paste; line endings of every
kind stored as `\n`; wrap on by default (`set_wrap`). `edit_key` answers
`KeyEdit` as `TextInput::edit_key` does, and leaves Tab and Ctrl+Enter to the
owner. The desktop's notes widget is the first user
(`gui/desktop/src/widgets.rs`, `note_press` / `note_key`), if an example of
the wiring helps.

## What happens until it is done

The apps work as they do now. The notes app's body stays append-only.
