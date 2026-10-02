### [E] Thirteen fields in nine applications are read and never filled, so the features behind them cannot be reached -- 2026-09-27

**Status:** FIXED or ANSWERED, every row, the same day -- see the last
column.
Found by `scripts/find-options-only-emptied.py`; a row fixed stops being
reported, and one answered goes into the script's `KNOWN` table.

**In short:** each of these is an `Option` the program reads -- to draw
something, or to decide what a key does -- and only ever clears. Nothing sets
it, so whatever it would show or allow never happens. It is the shape
`apps/kanban` had with `selected_card`, where it made every card operation
unreachable; here the losses are smaller and more scattered.

| App | Field | What cannot happen | Now |
|---|---|---|---|
| `diagram` | `rect_select_start`, `rect_select_end` | dragging a box to select several shapes: the rectangle is drawn from these and nothing starts one | FIXED: a drag on empty canvas draws the box and selects what it touches; dragging one of them moves them all |
| `filediff` | `dir_compare` | comparing two folders: the view draws a result nothing produces | FIXED: Ctrl+D asks for two folders and compares them on disk -- byte for byte within a 256 MiB budget, past which a pair is `NotCompared`, and a pair one side cannot read is `Unreadable`, never guessed; Enter opens a pair, Escape goes back |
| `filesearch` | `extension_filter` (the tests set it), `path_contains` | filtering results by extension or by a folder in the path | FIXED: `ext:pdf` and `in:Documents` in the query set them, and the status line says so |
| `ircclient` | `password` | joining a server that wants a password (`PASS`) | FIXED: `/connect server [port] [password]`; the history keeps stars |
| `magnifier` | `picked` (the tests set it) | picking a colour: the swatch and its values are drawn from it | ANSWERED: nothing can capture the screen, and picking refuses and says so |
| `paint` | `active_slider` | dragging the colour sliders | FIXED -- and it hid a larger hole: no colour could be chosen at all. The palette ignored clicks, and the colour dialog was opened only by a test. The tool panel is hit-tested now (tools, both swatches, the palette: left click the colour, right click the background) and the dialog works -- dragged sliders, Tab and the arrows, typed hex, Enter and Escape -- opened by the swatches or C / Shift+C |
| `screenrecorder` | `active_annotation_tool`, `current_annotation`, `hovered_sidebar` | annotating a recording, and the sidebar's hover | `hovered_sidebar` FIXED -- and the sidebar answered clicks no better: a click opens the view under it now, and the row under the pointer is lit. The two annotation fields ANSWERED: the toolbar that sets them is drawn only while recording, and recording refuses -- no frame source -- and says so |
| `videoplayer` | `audio_preferred_lang` | choosing an audio language: the preferences show "Any" for ever | FIXED, with its frozen twin `subtitle_preferred_lang` ("eng", drawn and read by nothing): Audio Language and Subtitle Language are rows stepping through sixteen languages and back to the file's own, and a file opens with the tracks in them -- matched by ISO 639-2 in either form (`deu`, Matroska's `ger`) or a BCP 47 tag (`de-AT`). The subtitle default is now the file's own: English, once something read it, would have turned on the subtitles of every film with an English track |
| `whiteboard` | `marquee` | dragging a box to select strokes | FIELD REMOVED: the box works through `DragState::Marquee`; this was a second copy only ever cleared |

**Why the tests did not notice:** where a test covers the feature it sets the
field itself first (two of the rows), which is the whole defect in miniature.

**The fix, per row:** give the field its writer -- the drag, the key or the
control the rest of the feature already assumes -- with a test that reaches it
the way a user does, then remove the row from this table; or, where the
feature should not exist, remove the field and what reads it.
