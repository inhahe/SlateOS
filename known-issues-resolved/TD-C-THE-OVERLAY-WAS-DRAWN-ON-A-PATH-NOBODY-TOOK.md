## `TD-C-THE-OVERLAY-WAS-DRAWN-ON-A-PATH-NOBODY-TOOK` -- **FIXED 2026-09-18** (lane C)

**In short:** `apps/diskimager` has two methods called `render`. One belongs to
the app itself; the other belongs to the trait the window system calls, and it
just turns around and calls the first. I put the new shortcut list in the
outer one. Everything still compiled, the app still worked, and the list was
invisible to every piece of code in the crate that draws the app directly --
which is all of its tests, and would be any embedder holding the concrete type.

**How it surfaced.** The draw test failed with `"Ctrl+1 / Ctrl+2" never
reached the window` while the key tests all passed. That pairing is the tell:
the state changed, the handler ran, and nothing was drawn -- so the defect is
on the drawing side, not the input side. Had I written only the key tests (the
obvious ones -- they are what the feature *is*) the app would have shipped with
a help key that opens nothing.

**Why this shape is worth a name.** It is the same defect the rest of this
app had, one level up. `verify_after_write` was a setting drawn on screen with
no way to reach it; the shortcut card was a drawing placed on a path nothing
reaches. In both cases the code is present, correct, and unreachable, and no
compiler or lint says a word -- the outer `render` really is called, by the
window system, in production. Only the tests take the other door.

**The generalisation, checked rather than assumed.** Seventeen apps have been
given a shortcut card. I checked every one of them for a second draw entry
point that skips the card, because if the shape recurred the other sixteen
would have shipped the same hole:

| App | Draw entries | Card reached by |
|---|---|---|
| calendar | `frame`, trait `render`, `Probe::draw` | all three -- the latter two call `frame` |
| imageviewer | trait `render` -> free `render(app)` | the one path |
| slides, mindmap, spreadsheet, renamer, logviewer, reminders, diagram | `render_commands` | the one path |
| pdfviewer, sudoku | `frame` | the one path |
| filediff | `render_tree` | the one path |
| explorer, hexeditor, jsonviewer, regextester | one `render` | the one path |
| **diskimager** | **inherent `render` + trait `render`** | **the inherent one, since this fix** |

So it was one app, not a pattern -- but the reason it was one app is that the
other sixteen happen to funnel every caller through a single function. That is
a property nothing enforces, and the next app to grow a second entry point
will reintroduce this silently. The durable guard is the draw test itself:
`the_shortcut_list_reaches_the_window` catches it in whichever app it happens
to, because the test drives the app the way the crate's own callers do.

**Fixed** in `96818ce94` by moving the call into the inherent render, ahead of
both dialogs, so both doors reach it.
