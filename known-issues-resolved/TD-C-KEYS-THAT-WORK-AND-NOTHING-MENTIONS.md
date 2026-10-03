## `TD-C-KEYS-THAT-WORK-AND-NOTHING-MENTIONS` -- **FIXED 2026-09-18** (lane C)

**In short:** Several keys added on 2026-09-18 to reach features that had no
way in are themselves undiscoverable. The feature is reachable now; finding it
still requires reading the source. That is better than before and is not
finished.

**Where each new key stands:**

| App | Key | Discoverable? |
|---|---|---|
| `rssreader` | eleven | **yes** -- `?` lists them, and a test asserts every listed row is answered |
| `passwordgen` | `L`/`U`/`D`/`S`/`A` | **yes** -- the options panel reads "Lowercase (L): Yes" |
| `metronome` | arrows, digits | **yes** -- "Practice Increment: +10 BPM (left/right)" |
| `hexeditor` | `Ctrl+I` | **yes** -- the search bar reads "Case: on  Ctrl+I" |
| `markdowneditor` | five | **yes** -- the panel's buttons were relabelled "Replace  Ctrl+Enter" |
| `slides` | `Ctrl+T`, `Ctrl+R` | **yes** -- "Theme: Mocha (Ctrl+T)", "Transition: Fade (Ctrl+R)" |
| `slides` | `S`/`O`/`L`/`A`/`I`, `Delete` | **yes** -- `?` lists all twenty, and a test asserts every listed key is answered |
| `pdfviewer` | `D` | **yes** -- `?` lists eleven, and the guard presses `on_event` -- four of them live a layer above `handle_event` |
| `calendar` | `W` | **yes** -- `?` lists twelve, and a `?` typed into the search box stays a `?` |
| `imageviewer` | `B`, `S` | **yes** -- `?` lists seventeen, drawn over the two bars those keys hide |
| `spreadsheet` | `Ctrl+T` | **yes** -- **F1** lists fifteen -- `?` is a character a spreadsheet must be able to type |
| `mindmap` | `B` | **yes** -- `?` lists seventeen |

**2026-09-18, later: the authoring keys were given the same treatment as they
landed**, so the three fixes did not widen this entry. `notes` says "No notes
yet -- Ctrl+N makes one." where the user is already looking, and its editor
placeholder reads "Select a note, then Enter to write in it"; `diagram`'s
properties panel reads "Label (F2)" for both a node and an edge. **The empty
state is the best place a primary action can be named** -- it is on screen
exactly when somebody wants to start and has nothing else to read.

Fixing that also turned up an asymmetry worth recording: `diagram` draws the
label in *two* property rows, node and edge, and the live-buffer fix had gone
into only the node one -- so an edge being relabelled showed the new text on
the canvas and the old text in the panel. No test covered edges to say so.
`a_user_can_label_an_edge` does now. **A fix applied to one of a pair is a
fix that looks complete from the diff.**

**2026-09-18, later still: `apps/slides` got the overlay.** It had the most
undiscoverable keys of any app here -- twenty bindings and, until `?` existed,
no way to learn one but reading the source, including the five that put shapes
on a slide, which are the ones somebody wants first. `SHORTCUTS` is twenty
rows; `?` toggles a card over the slide and `Escape` closes it;
`every_advertised_key_does_something` walks the list and presses each key.

**The guard test taught something the rssreader one had not.** Its first run
failed on the very first row -- `Left`, "Previous / next slide" -- and the app
was right and the test was wrong. `advance(-1)` at the first slide returns
`Ignored` on purpose, as does `1` when the edit view is already up, and
`Ctrl+V` with nothing copied. So the property a shortcut list actually claims
is **"some reachable state answers this key"**, not "this key is taken right
now": *declining from its own arm is answering*, and the defect the test exists
to catch is a row that falls through to the catch-all in every state. The test
now offers each key to three decks -- a fresh one, one in the sorter view, and
one mid-deck holding a copied slide and a selected text box -- and requires one
of them to take it. Between them every advertised key has something it could
do, which is the only reason three is enough.

**It was then mutated to check it could fail for its own reason**, since the
first failure had been about state rather than about a missing handler: one row
retyped from `B` to plain `C` (the app answers only `Ctrl+C`), which the test
rejected by name. A guard test that has only ever been green is a decoration.

**The event is derived from each row's key text rather than looked up in a
table beside it.** A parallel table would be a second list to keep in step --
the eleventh way a search says nothing, rebuilt inside the test written to
prevent it.

**The pattern that worked** is naming the key beside the thing it controls,
which costs one format string wherever the app already draws the value. It did
not apply to the last five: `pdfviewer` draws no reading-mode indicator,
`calendar` draws no week-start indicator, and **a toolbar cannot advertise the
key that hides it**, because once hidden the advertisement is gone with it --
which is the case `imageviewer` `B`/`S` and `spreadsheet` `Ctrl+T` all are.

**2026-09-18, last: the other five got an overlay, and this entry is closed.**
The reason it had been filed rather than done was that a status hint needs a
*new element* in a layout I had not read, and getting it wrong overlaps
something that was fine -- a worse defect than the one being fixed, and harder
to notice. **An overlay dissolves that objection entirely**: it is drawn last,
over everything, centred, and only when asked for, so it cannot disturb a
layout it does not understand. That was true the whole time the entry said
otherwise.

| App | Key | Rows | Note |
|---|---|---|---|
| `mindmap` | `?` | 17 | |
| `imageviewer` | `?` | 17 | drawn over the two bars `B` and `S` hide |
| `spreadsheet` | **`F1`** | 15 | `?` is a character a spreadsheet must be able to type into a cell |
| `calendar` | `?` | 12 | a `?` typed into the search box stays a `?` |
| `pdfviewer` | `?` | 11 | the guard presses `on_event`; four keys live above `handle_event` |

**Three things the work turned up that no plan predicted.**

`apps/spreadsheet` could not use `?` at all. Its key handler ends in a
catch-all that starts editing the cell on any printable character, so binding
`?` to help would have taken a character out of a program whose whole job is
holding characters -- a worse defect than the one being fixed, arriving by a
different road than the one I had been watching. `F1` there, and a test named
`a_question_mark_is_typed_into_the_cell_not_swallowed_as_help` to hold it.

`apps/pdfviewer` answers four of its keys one layer up, in `on_event` rather
than `handle_event`. A guard test pressing the inner function would have
reported `Ctrl+Q`, `Ctrl+F`, `Ctrl+T` and `Ctrl+W` as dead and invited somebody
to "fix" four keys that work. **The door a test presses is part of what it
tests**; this one presses `on_event`, which is where a real keystroke arrives.

And the overlays needed a *second* test each. `every_advertised_key_does_something`
reads the list against the handler; `the_shortcut_list_reaches_the_window`
reads it against the screen. `apps/netscan`'s `wol_note` was written by the
model and drawn by nothing for three commits with every model-level test
passing, so an overlay that never draws is a defect with a green suite behind
it. Both questions get asked in all seven apps that now have a list.

**Do not treat this as cosmetic.** `apps/slides` could add four shapes and an
image for hours before anyone found `S`, and the app it most resembles --
before this change -- was one that could only make decks of textboxes.
