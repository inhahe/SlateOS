## 1211. A snippet is written in the column it is shown in, and the library is kept like the notes library

**Date:** 2026-09-26
**Lane:** E
**Decided by:** Claude (autonomous) -- Claude's to revisit

**In short:** The snippets app could hold a library of code and could not
change a line of it: a new snippet was named from the search box and had no
content, and nothing could give it any. Now F2 (or the Edit button, or making
a new one) turns the right-hand column -- where the snippet is shown -- into
an editor for it: title, language, folder, tags, description, and the code.
Ctrl+S saves; leaving over changes asks first. The library is written to
`snippets/library.txt` in the settings folder after every change.

### Where the editor is

| | In the snippet's own column (chosen) | A dialog over the window |
|---|---|---|
| Room for the code | the whole column, as the code is shown | what a dialog can spare |
| Seeing the list while editing | yes, though it does not answer | no |
| Like the other editors here | the notes app edits a note where it is shown | the calendar and reminders use a form -- short fields, not a page of code |

### Smaller calls

- **Tab indents in the code**, as it does in any code editor; Shift+Tab leaves
  the code for the field above. Everywhere else Tab walks the fields.
- **New still makes the snippet at once**, named from the search box as
  before, and opens the editor on it: a New that opened an empty editor and
  made nothing until a save would lose the search-box naming the list's order
  depends on.
- **The editor is modal within the window**: while it is up, a press on the
  list or the sidebar does nothing, so a stray press cannot pick another
  snippet out from under an unsaved one.
- **A first run opens on the examples**, as before, and writes nothing until
  something changes (kanban's starting board, §1207).
- **Delete asks**: there is no undo, and a snippet can be a page of code.
- **Tags are one line, split at commas**, trimmed, empty ones and repeats
  dropped; the file keeps each on a line of its own, so a tag cannot break
  its line whatever it holds.

**Where it lives:** `apps/snippets/src/main.rs`: `library_text`,
`parse_library`, `Editing`, `SnippetField`, `App::{from_settings,
load_library, keep, request_close, answer, edit, save_edit, leave_editor,
handle_edit_key, press_in_editor, draw_edit_form, draw_edit_code,
draw_confirm_delete}`; `apps/textarea` (`TextArea::with_family`).

**How to reverse:** the format is `library_text`/`parse_library`; the editor
is drawn by `draw_editor`'s first branch.
