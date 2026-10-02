## `TD-C-NOTES-CANNOT-MAKE-A-NOTE` -- **FIXED 2026-09-18** (lane C)

**In short:** `apps/notes` starts empty and cannot create a note, title one, or
write a word in one. It can *export* notes -- to plain text, Markdown and HTML,
all reachable. The empty view says **"No notes yet."**, which reads as "you
have not made one", when the truth is that you cannot.

**Verified, including the routes that would have made it untrue.**

| | |
|---|---|
| `create_note` | **no production caller.** Both `notes.push` sites are inside it and inside `create_note_from_template`, which also has none |
| `update_note_content` | no production caller; it is the only caller of `set_content` |
| `update_note_title` | no production caller |
| import | **there is none.** The only file machinery is `export_plain_text`, `export_markdown`, `export_html` and `save_selected_note`, each reachable -- this app exports notes it cannot create |
| startup | `main` builds `NotesApp::new()`; `seed_sample_content` is called only from tests |
| what it types | `TextEntry` covers `Search`, `Tag(String)` and `NotebookName(String)` -- a note's body is not among them |

**Second instance of the shape filed an hour earlier for `apps/slides`**
(`TD-C-A-PRESENTATION-EDITOR-THAT-CANNOT-TYPE`), and the pair is the argument
that it is a class rather than an accident. Both are authoring programs. Both
have the whole periphery built -- notebooks, tags, versions, search, three
export formats, a note menu; slides has themes, transitions, a sorter view,
undo. Both are missing only the act the program exists for.

**"No notes yet." is the wrong sentence, by this lane's own rule.**
design-decisions 862 says an app that *cannot* obtain its data must say so and
an app that merely *has* none may stay quiet -- and it names `notes` in the
second group, on the grounds that "you can make a note, so empty honestly means
you haven't yet". **That was wrong about this app**, and the entry is being
corrected rather than quietly left: the premise was checked against
`TextEntry`, which does handle typing, but for tags and notebook names.
`apps/finance` gets this exactly right in a comment on its own empty state --
"the message implies data can be entered, which it cannot".

**I built a pointer layer for this app.** Four panels, hit-testing for notes,
notebooks, versions and tag chips, and seven of eleven operations made
reachable. All of it was real. None of it was the act of writing a note, and I
did not notice, because I was answering "which written operations have no
caller" -- and nobody had written note authoring for that question to find.

**What the repair wants.** `create_note` and `update_note_content` both exist
and both take what they need; the missing piece is a mode that routes
keystrokes into the selected note, on the pattern `apps/markdowneditor` now
uses. Until then the empty view should say what `finance`'s does -- that notes
cannot be created here -- because an empty list that invites you to add
something is worse than one that admits it cannot.


**Fixed the same day.** `Ctrl+N` asks for a title and makes the note,
`Ctrl+Shift+N` makes a notebook, and `Enter` on a selected note writes in it --
`Enter` inserting a newline there rather than committing, because a note is
more than one line and `Enter` is how you get the second one.

**Three decisions in it worth keeping:**

- **The body commits when the mode is left, including on `Escape`.** Losing
  what was typed because the exit key was the cancelling one is the worst
  thing a text editor can do, and `Escape` is how anyone leaves a multi-line
  box. Tested by `leaving_the_body_keeps_what_was_typed`.
- **The editor draws the live buffer, not the stored note.** The commit cannot
  happen per keystroke -- `set_content` snapshots a version on every call, so
  that would file one version per character -- which means the note holds the
  old text while typing, and drawing *that* would leave the user typing into a
  panel that never changes.
- **The first note creates a notebook to live in.** This app starts with no
  notebooks at all and `create_note` needs an id, so without it the first note
  anyone tried to make would have had nowhere to go -- and a refusal there is
  indistinguishable from the defect being fixed.

**The test that matters is `a_user_can_make_a_note_and_write_in_it`**, which
starts from an empty app and asserts the artifact comes out: Ctrl+N, a title,
Enter, a body of two lines, Escape, and then `note.content == "milk
eggs"`.
Deliberately end-to-end rather than "the key sets the field" -- every piece of
this existed already and the program still could not be used. 120 tests, up
from 116.
