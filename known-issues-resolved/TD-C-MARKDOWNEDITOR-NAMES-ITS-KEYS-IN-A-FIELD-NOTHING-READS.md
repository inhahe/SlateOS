## `TD-C-MARKDOWNEDITOR-NAMES-ITS-KEYS-IN-A-FIELD-NOTHING-READS` -- **FIXED 2026-09-21** (lane C)

**In short:** `apps/markdowneditor` answers ten keyboard shortcuts and writes
every one of them down in a place no user can ever see: the `tooltip` field of
its toolbar buttons. Nothing in the crate reads that field, and a tooltip needs
a hover, which this app cannot receive -- it handles no mouse event of any
kind. So the editor had bold, italic, link, find, undo and save on keys, and
told nobody. One of the keys it advertised there, `Ctrl+O`, was bound to
nothing at all.

**Why no gate caught it.** Three nearly did, and the gap between them is the
interesting part:

| gate | why it was silent |
|---|---|
| `check-fields-written-never-read.py` | reports fields only *tests* read. Nothing reads `tooltip` at all, which it leaves to `dead_code` |
| `dead_code` | the field is `pub` on a `pub` struct in a binary, so it is not dead by the compiler's reckoning |
| `key-survey.py` | counted the tooltip strings as the keys being named -- a string literal is not proof the string is drawn, which its own docstring says it cannot know |

The survey could not see the keys either, for a separate reason recorded as
shape 23: this app defines its own `Key` with the key in the payload
(`Char('h')`), and the survey read variant *names*. It reported a text editor
with 56 binding sites as having two unnamed keys.

**Fixed** by giving it the shape the other hundred apps use -- a `SHORTCUTS`
table, `F1`, and `guitk::shortcut::render_card` -- plus:

* **`Ctrl+O` now opens the file picker.** It was advertised and answered by
  nothing. The toolbar's Open button cannot be clicked either, so this was the
  only route left to a file that was not already open.
* **`handle_key` returns whether it answered the key.** It returned `()`, so
  "did this program answer this keystroke?" could only be inferred from some
  field moving -- and choosing that field is the mistake this tree has made six
  times, always by picking the observable whose name matches the verb. Now the
  guard asks the handler directly. It also stops a stray key repainting the
  whole document, which was a real if small bug.
* **No `?` row.** An unmodified character key inserts itself into the document
  here, so `?` would cost a question mark in a markdown file to buy a list `F1`
  already opens -- the second app where `?` was not free, after `apps/ebook`.

The card check sits above the find panel's branch, which returns before
everything below it. This is the third app where that placement was the
difference between a list and a modal with no exit.
