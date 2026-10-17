### [E] Twenty-eight applications keep a box's text twice -- 2026-10-16

**Status:** OPEN

**In short:** a one-line text box in these programs keeps what was typed in
two places: a plain `String` the program reads, and the toolkit's editor
(`TextInput`, the caret and the selection) holding a copy of the same text.
Before every key, press and menu row the program compares the two and reloads
the editor from the `String`. Nothing is wrong that a user sees today; the cost
is that every program carries its own copy of the reload, the key routing and
the press, and each copy is a place for the two to disagree -- which is how
calendar and reminders lost F1 while searching.

**Where.** Found on 2026-10-16 by searching `apps/*/src` for an editor compared
with the text it is laid over (`<editor>.text() != <text>`, `fn sync_*_editor`):

| Application | Box |
|---|---|
| clipmanager, contacts, dbviewer, mindmap, netmanager, netscan, qrcode, startupmanager, taskscheduler, vpnmanager | `editor` |
| colorpicker | `hex_editor` |
| defrag | `exclude_editor` |
| dictionary, filesearch | `query_editor` -- a search, of the shape the twenty-four search boxes had |
| diskanalyzer | `path_editor` |
| editor | `find_editor` (`apps/editor/src/input.rs`), the find bar's two fields |
| hexeditor | `box_editor` |
| ircclient, kanban | `input_editor` |
| lockscreen | `password_editor` |
| passwordgen | `analyzer_editor` |
| photomanager | `entry_editor` |
| pdfviewer | the print range's box (`edit_box`, `apps/pdfviewer/src/main.rs`) |
| rssreader | the prompt's box (`entry_editor`, `edit_box`) |
| screenshot | `annotation_text_editor` |
| stickynotes | `line_editor` |
| systemrestore | `form_editor` (`sync_form_editor`), the schedule form's fields |
| whiteboard | `text_editor` |

**The fix.** The twenty-four search boxes moved onto `textline::SearchBox`
(`apps/textline/src/search.rs`, roadmap "One search box, not twenty-four
copies of it"), in which the editor *is* the text: `key` says whether the box
took the key and whether the text changed, and `press`, `menu_row` and `draw`
are the box's. Nothing in it is particular to searching except its name. The
fix is to name it for what it is -- a one-line box (`LineBox`) -- and move
these boxes onto it the same way, each with its tests kept. A box that is one
of several fields sharing one editor (editor's find bar, systemrestore's form)
holds one box per field instead. dictionary and filesearch are searches and
could move as the twenty-four did.
