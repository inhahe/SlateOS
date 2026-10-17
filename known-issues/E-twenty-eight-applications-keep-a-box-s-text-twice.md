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

**A second defect rides with it: twenty-three keep a clipboard of their own.**
All but qrcode, systemrestore, pdfviewer, rssreader and lockscreen hand
`textline::apply_key` a `String` of the program's own as its clipboard, not
the program's clipboard (`guitk::clipboard`, which `apply_key_on_clipboard`
and every right-click menu use). What Ctrl+C takes in such a box is not on
the clipboard the toolkit's own fields and menus read, and what they copy
cannot be pasted into it.
editor's is also the document's copy and paste, and colorpicker's is what its
Copy-colour button fills. Each moves onto `guitk::clipboard` as its box moves.
lockscreen pastes nothing, and stays so.

**What the boxes need** (surveyed 2026-10-17, box by box):

| Need | Applications |
|---|---|
| One editor shared by several strings, swapped as the focus moves: one box per string instead | clipmanager (4), contacts (16), dbviewer (2), mindmap (2), netmanager (4), netscan (5), qrcode (8), startupmanager (6), taskscheduler (3), vpnmanager (6), editor (2), hexeditor (2), photomanager (2: one inside `TextEntry::AlbumName`), stickynotes (the search and each note's title), systemrestore (2) |
| A font size that changes after the box is made, or per box | dictionary (follows the window's height), mindmap (node size × zoom), stickynotes, clipmanager, dbviewer, photomanager |
| Bold text | mindmap (the root), passwordgen, stickynotes |
| A check that refuses an edit whole, or rewrites the text | colorpicker (hex digits and `#`, upper-cased; a paste of anything else refused), taskscheduler (digits within a maximum), vpnmanager (digits, clamped to 65535) |
| Masking, and switching it | passwordgen (hidden until Ctrl+R), lockscreen (drawn as dots of its own, a press to the nearest dot, no paste) |
| The caret put at a byte | stickynotes (Up from the body, a double-click), lockscreen |
| Drawn unfocused from its end, elided at the start | diskanalyzer (a path) |
| Drawn by the program, from `input()` | dbviewer's SQL line (coloured spans), stickynotes' titles (on the note, "[P] " when pinned), lockscreen's dots |

A placeholder shown only while the box lacks the keyboard (netmanager,
pdfviewer, rssreader, passwordgen, qrcode) needs nothing: the program passes
an empty one while it has it.

**The fix.** The twenty-four search boxes moved onto `textline::SearchBox`
(now `LineBox`, `apps/textline/src/linebox.rs`; roadmap "One search box, not twenty-four
copies of it"), in which the editor *is* the text: `key` says whether the box
took the key and whether the text changed, and `press`, `menu_row` and `draw`
are the box's. Nothing in it is particular to searching except its name. In
this order, a commit each:

1. Rename it for what it is: `LineBox`, `LineKey`, `LineLook`, in
   `apps/textline/src/linebox.rs`; the twenty-four search boxes follow.
   *Done 2026-10-17.*
2. Give it the needs above, each with its tests: `set_font_size`, a weight,
   `set_caret`, a check an edit must pass (`key_checked`), a mask, and a
   look that elides at the start.
3. Move the twenty-eight, one application at a time, each with its tests and
   its `mutate.py` rows kept, and its clipboard on `guitk::clipboard`.
