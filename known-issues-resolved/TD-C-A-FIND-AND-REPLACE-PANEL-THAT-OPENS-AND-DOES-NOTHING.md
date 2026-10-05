## `TD-C-A-FIND-AND-REPLACE-PANEL-THAT-OPENS-AND-DOES-NOTHING` -- **FIXED 2026-09-18** (lane C)

**In short:** In `apps/markdowneditor`, Ctrl+H opens a Find & Replace panel.
The panel has a "Find:" box, a "Replace:" box and three buttons. **Nothing you
can do affects any of them.** No keystroke reaches either box, the buttons
cannot be clicked because this app has no pointer handling at all, and the four
operations behind them have no callers. Escape closes it again. That is the
whole of the feature.

**Verified, and the panel really is reachable** -- this is not a dead dialog:

| | |
|---|---|
| opening it | `Key::Char('h')` with ctrl toggles `find_state.visible`; `Ctrl+F` sets it true |
| drawing it | `render_find_replace` runs whenever `find_state.visible`, drawing `Find:`, `Replace:` and `["Replace", "Replace All", "Close"]` |
| typing into it | **nothing in production writes `find_state.query` or `find_state.replacement`** -- both are permanently `String::new()` |
| the buttons | `markdowneditor` handles no pointer events, so no click can reach them |
| the operations | `replace_current`, `replace_all`, `next_match`, `prev_match` each appear exactly once in the production half: their own definition |
| `find_state.case_sensitive` | frozen as well -- see `TD-C-SETTINGS-THE-PROGRAM-OBEYS-AND-NOTHING-CAN-CHANGE` |

`go_to_line` is unreachable by the same measure, and the toolbar's "Find"
button carries the tooltip "Find & Replace (Ctrl+H)" -- which is the one part
of this that is true.

**Why this is worse than an absent feature.** An editor with no find is an
editor you search by eye. An editor that *opens a find panel* has told you the
feature exists, so a user who cannot make it work concludes they are using it
wrong -- and a keyboard-only app gives them every reason to think so, since the
obvious move is to click a field that cannot be clicked. **The failure is
silent, and it is attributed to the user.** That is why nobody has reported it.

**Three defect classes meeting in one panel**, each already filed separately:
operations with no caller, fields the program obeys that nothing can set, and a
UI drawn for a pointer in an app that handles none. Any one of them alone would
leave a usable program.

**What the repair wants.** The panel needs the treatment `apps/rssreader` just
had: a text-entry mode routing keystrokes into `query` and `replacement`, a key
to move between the two boxes, Enter for next match, and keys for Replace and
Replace All -- then `replace_current`, `replace_all`, `next_match` and
`prev_match` have callers and the buttons become labels for keys rather than
targets for a pointer that does not exist. `go_to_line` wants the same kind of
small prompt. **Do not start by adding pointer handling:** the panel is one
feature of a program that is keyboard-driven throughout, and giving this one
dialog a mouse would make it the only part of the app that needs one.

**Method note.** I twice nearly got this wrong, in opposite directions. First I
concluded Ctrl+H was unbound, having grepped `Key::H` when the app spells it
`Key::Char('h')`. Then I nearly filed the panel as dead code, having grepped
`find_state.query` when the render function takes the struct by reference and
spells it `state.query`. **Both were the same mistake: searching for one
spelling of a thing and reading the silence as absence.** The check that
settled it was for *writers* of the field under any receiver, which is the
question that was actually being asked.



**Fixed the same day.** `handle_find_key` routes the keyboard into the panel
while it is open: letters reach the focused box, `Tab` moves between Find and
Replace, `Enter` and `Shift+Enter` walk the matches, `Ctrl+Enter` replaces and
`Ctrl+Shift+Enter` replaces every match, `Escape` closes. The panel takes every
key while it is up, for the reason every text mode in this tree does -- a query
containing `s` must not save the document behind it, which there is now a test
for.

`Enter` also puts the *cursor* on the match rather than only counting it, which
is the half of "find" that is not searching -- and is the only caller
`go_to_line` has ever had. The three buttons now read "Replace  Ctrl+Enter",
"Replace All  Ctrl+Shift+Enter" and "Close  Esc": in an app that handles no
pointer events a button can only ever tell you what to press, so it should say
so. `find_state.case_sensitive` remains frozen and stays filed under
`TD-C-SETTINGS-THE-PROGRAM-OBEYS-AND-NOTHING-CAN-CHANGE`; the panel draws no
control for it, so it is a smaller matter than the rest. 225 tests, up from
217.
