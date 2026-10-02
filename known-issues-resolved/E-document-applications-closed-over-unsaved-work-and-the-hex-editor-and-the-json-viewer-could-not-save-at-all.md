### [E] Document applications closed over unsaved work, and the hex editor and the JSON viewer could not save at all -- 2026-09-25
**Status:** FIXED for the text editor, the markdown editor, the hex editor, the JSON viewer, slides and sticky notes (lane E, 2026-09-25), and paint, the diagram editor, the whiteboard and the spreadsheet (2026-09-25). What remains is the entry below it: notes, contacts, snippets and kanban keep nothing at all.

**In short:** closing the window of an editor threw away every unsaved change
without a word -- the window library closed a window on any close request,
whatever the application answered, until lane F added `Response::KeepOpen`
(`requests/e-f-let-an-application-decline-a-close-so-it-can-ask-about-unsaved-work.md`).
Worse, two of the editors could not save at all: the hex editor's toolbar drew
a Save button (and New, Open, Undo, Redo, Find and GoTo) that answered nothing,
and no key saved; the JSON viewer keeps a "modified" mark and has no save. So
every edit either program made was lost when its window closed.

**What changed.** The markdown editor answers `KeepOpen` while its Save / Don't
save / Cancel question is up (it was drawn into a window already gone). The
text editor asks at all -- closing a modified tab had been refused with a
message offering Ctrl+Shift+W to discard, a key nothing bound. The hex editor
saves (Ctrl+S, Ctrl+Shift+S, the toolbar), atomically through `safeio`, and
**refuses to save a file it read only in part** over the file itself -- it reads
the first 16 MiB of a larger file, and writing those back would cut the file
short; Save As writes them to a new file instead. Its toolbar and tabs answer
the pointer, and closing a tab or the window over unsaved work asks, with
Save as file for a document that has none. Its documents now keep the real
path they came from, not the lossy display string, so a file whose name is not
valid UTF-8 is saved to itself rather than to some other name.

**Where.** `apps/editor/src/{main,input}.rs` (`CloseScope`, `answer_close`,
`close_prompt_key`), `apps/markdowneditor/src/main.rs` (`GEvent::CloseRequested`),
`apps/hexeditor/src/main.rs` (`save_active`, `save_to_own_file`, `picked`,
`TOOLBAR_BUTTONS`, `tab_rects`, `request_quit`). Mutation tables: `apps/editor/mutate.py`
(new), `apps/markdowneditor/mutate.py`, `apps/hexeditor/mutate.py` (new).

**The JSON viewer, the same day.** It saves (Ctrl+S, Ctrl+Shift+S, a Save
button) through `safeio`, refuses to save a partly read file over itself as
the hex editor does, and asks before a modified tab or the window closes. The
close question and the picker name their document by `Document::id` rather
than by position -- the arrangement `TD-C-JSONVIEWER-CAN-EDIT-A-VALUE-BUT-NOT-ADD-ONE`
warned about, and the first code to hold a tab across user interaction. Save As
starts beside the document's own file. Looking at it turned up six more
faults, all fixed in the same change:

| what | what the user saw |
|---|---|
| the toolbar (New, Search, Edit) sat above a click handler that began at the tab bar | three buttons that did nothing; Open and Save were not there to draw |
| a tab's "x" close mark was part of the tab's select area | clicking it selected the tab it was meant to close |
| `close_tab` never moved the active index when a tab *before* it closed | the next click-to-close would have shown a different document as the active one |
| the find bar lay over the tree and took none of its clicks | a click on the bar -- or on its "Aa" -- selected a tree row under it; "Aa" and "Esc" could not be clicked |
| an edit in progress was only a *path*, and survived a tab switch or close | Enter wrote the typed value into the next tab's document at the same path |
| a tree edit rewrote the whole text as `format_json(value, indent)`, `indent` being the raw view's two-space default | changing one number in a one-line or four-space file reformatted all of it -- harmless while nothing could save, a rewrite of the user's file once something could |

Also: the redraw fingerprint gained the tab list and the status line, so a
save (which changes neither content nor selection) and closing the first of
two fresh tabs (which left every other field equal) redraw; and the find bar's
matches are recomputed for the tab on screen instead of kept from the last
one. A tree edit now keeps the text's layout -- one line stays one line, an
indented file keeps its indent ([`IndentStyle::detect`]), a final newline is
kept or left off -- and the raw view opens in the file's own indent. Mutation
table `apps/jsonviewer/mutate.py` (new, 24 rows).

**Slides and sticky notes, the same day.** `apps/slides` asks before its
window closes over unsaved changes, with the question it already asked before
Open -- which now offers Save as well: S saves (asking where for a deck with no
file) and goes on only if the save worked, D goes on without saving, any other
key keeps the deck. A close during a slide show ends the show, which draws
nothing but the slide. Words being typed into a box when the window closes are
committed first, so they are asked about rather than dropped -- and a box
clicked into and out of again, unchanged, no longer marks the deck unsaved
(every finished edit counted as a change, and a box still showing its prompt
was emptied). `apps/stickynotes`: **Ctrl+Q quit without saving at all**, so
whatever had been typed since the last autosave went with the window; and the
close button saved and quit whether or not the save worked. Both now save
first, and a failed save keeps the window open once, with the reason on the
toolbar -- asking again quits, so a disk that stays broken cannot make the
window impossible to close. Mutation tables: `apps/slides/mutate.py` (eleven
rows new or rewritten), `apps/stickynotes/mutate.py` (new, 5 rows).

**Paint, the same day.** Nothing recorded whether the picture had changed,
so nothing could ask: the window closed over it, and Ctrl+N and Ctrl+O
replaced it, without a word. It keeps that record now (every edit goes through
`push_history`, which sets it; so do undo and redo, and the layer operations,
which do not go through history), marks the window bar with `*`, and asks --
on `apps/unsaved`, the first program to -- before a close, a New or an Open.
Ctrl+S writes over the picture's own file once it has one (it asked where
every time), Ctrl+Shift+S asks, and the picker starts beside the file. And
what an open or a save did is drawn in the status bar: it was recorded and
drawn nowhere, so a save that failed looked like one that worked. Mutation
table `apps/paint/mutate.py` (new, 13 rows).

**The diagram editor and the whiteboard, the same day.** Neither could save
anything it could open again -- the diagram wrote an SVG, or a JSON that kept
the shapes and dropped their colours, borders, fonts, arrowheads, layers and
groups, for "an importer that does not exist yet"; the whiteboard wrote the
page in front as an SVG, so a board's other pages could not be kept at all --
and each said so in a banner across the top of the window. Each now has a
file of its own, YAML as `apps/slides` keeps a deck (`.diagram`,
`.whiteboard`; `slateos-diagram: 1`, `slateos-whiteboard: 1`), written through
`safeio` and read back whole: every property, a later format refused rather
than half-read, a file cut short refused rather than read as a smaller
document, ids that clash refused, and a shape of an unknown kind -- or an arrow
to a box that is not there -- left out while the rest is read, the opening
saying how many were left out. Ctrl+S saves (asking where the first time),
Ctrl+Shift+S saves as, Ctrl+O opens, and the old save is Ctrl+E, export, which
is not a save: it leaves the unsaved mark.
Each records unsaved changes and asks on `apps/unsaved` before a close or an
Open, and the status line shows what the last save did (neither drew it).
Also fixed in the diagram: naming a box and leaving the name as it was no
longer counts as a change. In the whiteboard, two faults in undo, both of
which the file made matter: **a deletion could not be undone** -- the undo
record held only the shape's id and was made after the shape was gone, so
undo found nothing to put back (a layer's deletion likewise) -- and **the
history was one for the window**, replayed onto whichever page was showing,
so undo after switching pages took a same-numbered shape off the wrong page.
Deletions now carry what they took and where it was, and each page keeps its
own history. Mutation tables: `apps/diagram/mutate.py`,
`apps/whiteboard/mutate.py` (both new).

**The spreadsheet, the same day.** Ctrl+S wrote the sheet in front as CSV --
its values, with no formula, no format and no other sheet -- and Ctrl+O read a
CSV into the sheet in front. It now keeps a workbook, `.spreadsheet`: every
sheet, every cell as it was typed (a formula comes back a formula, and is
worked out again), every format, width, height and frozen pane, in a
tab-separated file read whole or not at all, a refusal naming the line
(design-decisions §1204). Ctrl+S saves (asking where the first time), F12
saves as (Ctrl+Shift+S was already the status bar's), Ctrl+O opens a workbook
-- or a CSV, as a new workbook of one sheet that does not take the CSV as its
file -- and Ctrl+E exports the sheet as CSV, which is not a save. Unsaved
changes are recorded where every change already went, the undo manager, plus
freezing panes, which undo does not see; the window bar shows `*`; and a close
or an Open over them asks on `apps/unsaved`, committing a value half typed
first. Mutation table `apps/spreadsheet/mutate.py` (new).

Four more that looked like the same case are a worse one -- see `[E] Notes,
contacts, snippets and kanban keep nothing` below.

**One question, not thirteen -- done the same day.** The six applications
fixed first each drew the question by hand, beside the toolkit's own
`guitk::modal::AlertDialog`, which has focus, hover, Escape, a scrim and the
destructive colour for the one button that loses work. `apps/unsaved` is that
dialog asked the one way -- Save, Don't save, Cancel; S, D, Escape, Tab; a
click beside the card answers nothing; shown at once rather than faded in, for
the applications that have no clock -- and paint, the text editor, the
markdown editor, the hex editor, the JSON viewer and slides all ask through it
now. What changed for a user: slides' Y ("yes, go on") and the markdown
editor's C ("cancel") are gone in favour of the shared keys, and a key that
answers nothing is swallowed rather than read as Keep. Mutation table
`apps/unsaved/mutate.py` (7 rows); each application's table follows its own
routing to the question.

**Left behind on purpose:** the two editors' *other* modal question -- "the
file changed on disk" -- is still drawn by hand in both. It has four answers
and a merge review behind one of them, which is more than a dialog's row of
buttons; it is the next candidate for the same treatment, not a reason to
have left this one hand-drawn.
