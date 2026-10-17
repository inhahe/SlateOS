### [E] Twenty-eight applications keep a box's text twice -- 2026-10-16

**Status:** OPEN -- fixed on lane-e (the last box moved in 6d2d0c868),
awaiting a boot on main

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
| Bold text | mindmap (the root), passwordgen, stickynotes -- met, `with_weight` |
| A check that refuses an edit whole, or rewrites the text | colorpicker (hex digits and `#`, upper-cased; a paste of anything else refused), taskscheduler (digits within a maximum), vpnmanager (digits, clamped to 65535) |
| Masking, and switching it | passwordgen (hidden until Ctrl+R), lockscreen (drawn as dots of its own, a press to the nearest dot, no paste) -- met, `masked` and `set_mask` |
| The caret put at a byte | stickynotes (Up from the body, a double-click), lockscreen -- met, `put_caret` |
| Drawn unfocused from its end, elided at the start | diskanalyzer (a path) -- met, `from_its_end` |
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
   look that elides at the start. *Each lands with the first application
   that uses it, so none is built without a caller.* Done so far:
   `LineKey::moved`, whether a key moved the caret or the selection, which
   eight programs had computed beside the box; `set_font_size`, for a
   box sized with its window (dictionary); `LineBox::holding`, a box
   made with something in it; `LineBox::key_checked`, a key whose
   edit a check must accept or it is undone whole (vpnmanager's number
   boxes); `LineBox::insert`, text the program decides typed at the caret
   (colorpicker's hex, filtered and upper-cased);
   `LineBox::from_its_end`, a box cut at its front without the keyboard
   and pressed where that was drawn (diskanalyzer's path);
   `LineBox::with_weight`, a box drawn, pressed and stepped through in a
   weight (mindmap's root, passwordgen's analyser); `LineBox::masked`,
   `set_mask` and `mask`, a box drawn as a mask for each character,
   pressed where the masks were drawn, and giving nothing to the clipboard
   (passwordgen, lockscreen) -- its menu still offers Cut and Copy, which
   do nothing, as no program shows a masked box's menu yet; and
   `LineBox::put_caret`, the caret at a byte the program measured itself
   (lockscreen's dots) (2026-10-17).
3. Move the twenty-eight, one application at a time, each with its tests and
   its `mutate.py` rows kept, and its clipboard on `guitk::clipboard`.
   *Moved (2026-10-17), all twenty-eight:* rssreader's prompt,
   pdfviewer's page range, systemrestore's form (two boxes), defrag's
   exclude pattern, screenshot's text annotation, whiteboard's words,
   ircclient's message line, kanban's input dialog, filesearch's query (in
   its criteria), hexeditor's find and go-to boxes (two), dictionary's
   query, photomanager's search and album name (two), netscan's five,
   startupmanager's search and dialog (six), clipmanager's four, qrcode's
   eight, netmanager's address and DNS boxes (four), vpnmanager's search,
   split-tunnel and dialog boxes (six), taskscheduler's form (five: the
   Param box is whichever of three its frequency asks for), colorpicker's
   hex box (its Copy button now fills the program's clipboard, which the
   box pastes from), diskanalyzer's path, mindmap's name and search,
   dbviewer's query and filter value, passwordgen's analyser (masked until
   Ctrl+R), lockscreen's password (masked, pasting nothing), contacts'
   search and the form's fifteen lines, editor's two find fields --
   whose document's copy and paste moved onto `guitk::clipboard` with them,
   so a word copied from the text still pastes into the search -- and
   stickynotes' search (its `NoteStore` filters by the box's text, handed
   to it) and the title being typed (a box of its own, filled from the
   note as the keyboard moves to it, written into the note on each edit,
   and filled again when an undo or a tag taken out of it changes the
   title under it). A row
   that stood for the second copy -- a reload, a write-back, a private
   clipboard, a caret or selection drawn by hand -- goes with it; the rest
   move onto the box. Drawings changed: hexeditor's box
   without the keyboard is drawn from its start, as every LineBox is, not
   scrolled to its end, and qrcode's placeholder sits on its text's line,
   3 px higher; diskanalyzer's path, pressed without the keyboard, is
   measured against the `…` and tail it shows rather than the path
   scrolled to its end, which put a press an ellipsis's width off;
   contacts' and editor's boxes without the keyboard are drawn from their
   start, cut with `…`, and pressed where they read -- a press there was
   measured against the text scrolled to its end -- and passwordgen's
   analyser without the keyboard is drawn from its start. A box the
   keyboard moves to types at its own caret, not where the shared editor
   last stood. lockscreen's read-out counted the password's bytes as its
   characters, and its `type_char` its capacity in bytes; both count
   characters now. Where
   a box's text is a model's data -- qrcode's WiFi and contact codes,
   compared and kept in history -- the box is the record
   and the model is made from it when it is needed
   (`QrApp::wifi_config`, netmanager's `IpEdit::to_config`, vpnmanager's
   `ProfileEdit::to_profile`); a dialog's state that only held its boxes
   drops its equality instead (startupmanager). taskscheduler's text
   field had lost the guard that draws its text only where the line fits,
   unseen because its containment sweep read neither `RichText` nor
   lines; both are read now.
