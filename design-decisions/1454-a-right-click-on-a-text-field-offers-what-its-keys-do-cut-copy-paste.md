## 1454. A right-click on a text field offers what its keys do: Cut, Copy, Paste

**Date:** 2026-10-01 &middot; **Decided by:** Claude (autonomous) &middot;
**Lane:** C

**In short:** Right-click in a text box -- the Run box's line, the start
menu's search, a note on the desktop, an icon's name while you rename it --
and a small menu offers Cut, Copy, Paste, Delete and Select all, and Undo
and Redo where the box remembers its changes, as a note does. A row that
would do nothing just now is shown greyed rather than left out. Programs
get the same menu from the toolkit for their own boxes.

**What was there.** The toolkit's fields had the clipboard keys (Ctrl+X, C
and V, since 2026-09-27) and, since 2026-09-30, one clipboard shared by
every field of a program (`guitk::clipboard`); nothing offered them to the
pointer (`roadmap-detailed.md` -> *Input Fields*: "No right-click context
menu anywhere").

**How it works:**

| | |
|---|---|
| The rows | `guitk::editmenu::rows(EditState)`: Undo and Redo (a field with a history), Cut, Copy, Paste, Delete, Select all, each with its keys beside it |
| Their ids | `EditCommand::id`, far up the id space (`0xED17` in the top bits), so a window can put rows of its own beside them without renumbering either |
| A field's part | `TextInput::edit_menu` / `edit_command`, `TextArea::edit_menu` / `edit_command`: the field says which rows can act, and does what a row says as its key would. It draws nothing and holds no menu -- the window that draws the field puts the menu up |
| The code editor's | `CodeView::edit_menu` / `edit_command`, the same pair. Its Cut and Copy are lit with nothing selected (`EditState::copies_line`), because Ctrl+X and Ctrl+C take the caret's line then; the menu's Delete takes only what is selected (`CodeEditor::delete_selected`), where the Delete key would take the character after the caret |
| The shell's part | one menu for its four fields, `DesktopShell::field_menu`, captured with the field it was opened on (`MenuField`) as the pin menu captures its row |

**Choices:**

| Choice | Taken | Alternative | Why |
|---|---|---|---|
| A row that cannot act | shown, greyed | left out | the menu keeps one shape to learn; the Windows edit control, GTK and Qt all grey |
| The caret, on a right-click in a field already being typed in | stays where it was | moves to the press, as a browser's does | Cut and Copy act on the selection the user made, and a right-click that moved the caret would throw it away first; the Windows edit control and Qt leave it too. A note not yet open opens with its caret at the press, since it must go somewhere |
| The Run box and the start menu | stay open under the menu | close, as opening any other menu closes them | the menu is about their field; closing them would leave it about nothing |
| A note's menu | its edit rows, then the widget's "Remove this widget" and "Remove all widgets" | the edit rows alone | a right-click on a note offered removing it before it offered anything else; its title bar still offers the widget menu alone |
| Typing after a row chosen on a note's menu | the note's, though it arrives on the popup surface the menu was drawn on | only keys arriving on the desktop's own surface | the compositor leaves the keyboard on the surface clicked; an icon's rename already had this exception, for "Rename" chosen from a menu |

**Not yet.** Pasting what another program copied: the clipboard is each
program's own until open question C-Q29 is answered. The applications'
fields are lane E's to wire
(`requests/c-e-text-fields-have-a-right-click-menu.md`).
