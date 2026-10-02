### [E] Warnings drawn where the next thing drawn covers them -- 2026-09-25
**Status:** FIXED (lane E, 2026-09-25) -- all fifteen apps.

**In short:** a sweep in mid-September gave many apps one or two lines of
text at the very top of the window saying something the user must know: the
alarm clock makes no sound, so "if this window is not in front of you, nothing
will wake you"; the calendar and the reminders list keep nothing; the
spreadsheet opens on an example. Each was drawn "after the background, or it
would be painted over" -- and then the app drew its toolbar or tab bar over the
same pixels. The lines were in every frame and on no screen. Their tests read
the frame's list of texts, where the lines were, and passed.

**Fixed** in `apps/notes` (drawn before the window's own background),
`apps/kanban` (under the toolbar, and on every board) and `apps/alarmclock`
(under the tab bar -- the worst of them, since an alarm clock that cannot wake
anyone must say so where it is read). Each now draws its lines where nothing
covers them, and its test also asks that nothing drawn after a line fills the
point it is drawn at.

The spreadsheet's lines were drawn before the window's background, on every
sheet, and said Ctrl+S wrote a CSV and that work was gone at close -- both
false since it kept workbooks and asked before losing one (design-decisions
§1204). One true line now, on the status bar, while the sheet is the example
nobody has touched.

**The other eleven, the same day** -- each marked by the same comment. A test
in each (`the_warning_lines_are_not_painted_over`) found eight painted over
(`calendar`, `credmanager`, `filediff`, `mindmap`, `musicplayer`, `podcast`,
`remotedesktop`, `videoplayer`) and, once it also asked that no other text share
a line's row, two more drawn under other text (`clipmanager`'s under its search
bar, `startupmanager`'s under its header; `reminders`' lines sat under its
header's title too, though the default theme draws that header unfilled). Each
got a place nothing else is drawn: a strip of its own under the top bar that
the content starts below (`calendar`, `credmanager`, `mindmap` -- gone since,
with what it warned of (§1208) -- `musicplayer`, `remotedesktop`, `reminders`
-- while the notice is shown, where it is keyed on an empty list), a strip along the bottom (`podcast`, which has no top bar),
or the empty panel the lines explain (`filediff`, `clipmanager`,
`startupmanager`, `videoplayer`), wrapped there rather than cut where the panel
is narrow. `videoplayer`'s empty picture also said "Ctrl+O to open", and Ctrl+O
is bound to nothing: the true lines are there now. The test in each asks that
nothing drawn after a line fills the point it is drawn at, and that no other
text overlapping it horizontally is on its row.

The calendar's and the reminders list's lines say they keep nothing, which is
so: two more apps that keep nothing, beside the four the entry below names --
and neither can make an item yet (the calendar only imports `.ics`, the
reminders list only opens JSON), so each wants a way to add one before a store
is worth much. Lane E's.

**The calendar, 2026-09-26.** Events are added (N, or New event in the top
bar), changed (Enter, or a second press on an event) and deleted (Delete, which
asks) in a form, and kept in `calendar/events.txt` in the settings directory,
written after every change and read whole or not at all (design-decisions
§1209). Its empty line now says how to add one; what the last import or export
did has a line of its own under the top bar, where it had been drawn across the
top bar's buttons.

**The reminders list, the same day.** Reminders are added (N), changed (E) and
deleted (Delete, which asks) in a form, from the keyboard like the rest of the
program; their steps -- which the model had and nothing could reach -- are
added, ticked and taken off in the form, and Shift+1-9 ticks one from the list.
The list is kept in `reminders/tasks.txt` in the settings directory (§1210).
Found on the way: a repeating reminder, once done, was finished for good -- it
now comes round at its next time still to come -- and the snooze question and
what the last save did were drawn at the foot of the window, over the list's
last row; they have lines in the strip under the header now.
