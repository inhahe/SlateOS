## 811. An application's window title is re-read every batch, and a spreadsheet sort aimed at one cell covers the block around it

**Date:** 2026-09-04. **Decided by:** Claude (autonomous).
**Lane:** C

**In short:** Two small choices made while connecting `apps/spreadsheet` to a
window. First: until today, the words in a window's title bar were read from the
program once, when the window opened, and never again -- so a program whose
title named the file it was editing showed the *first* file for ever. That is
now re-read as the program runs. Second: in the spreadsheet, clicking the "A-Z"
sort button while a single cell was selected did nothing at all, because the
program sorted exactly the cells you had selected and one cell is already in
order. It now sorts the run of filled rows around that cell.

### The title

`oswindow::app::App::title` was documented as read-once, with the reasoning
written into the trait: making it live "would mean re-reading it on every event
to find out whether it had changed, which is a round trip per mouse move to
answer no".

The premise is wrong on the facts. Re-reading is a local call that builds a
`String`; only a *difference* costs a round trip, because `set_title` is sent
only when the new title differs from the recorded one. And the cost is per
*batch*, not per event -- a drag of two hundred mouse moves is one batch
boundary, so it is one `String` against a frame that has just been laid out and
serialised.

What the old rule actually bought was a fleet of programs whose titles were true
for one instant. `apps/slides` -- wired an hour before this entry was written --
opened saying "slide 1 of 6" and went on saying it on slide six. Its test
asserted that the title followed the current slide, and passed, because the test
called `title()` itself. That is this window's recurring shape: a check that
runs, passes, and is about the wrong thing.

| | *What changes* |
|---|---|
| Read once (before) | The title names whatever the document was at startup. Cheapest possible; wrong for nearly every app. |
| Re-read per batch (now) | The title follows the document. One `String` per batch; one round trip per actual change. |
| Re-read per event | Same result, hundreds of times the cost during a drag, for no additional correctness. |

A failure to rename is deliberately *not* fatal, unlike the frame path. A window
that cannot be drawn has nothing to show; a window that cannot be renamed still
works, and killing the program over the label above it would trade a cosmetic
fault for a lost document.

### The sort

`SpreadsheetApp::sort_column` sorted `selection.primary_range()`, and
`Sheet::sort_by_column` returns immediately when `start_row >= end_row`. A
single-cell selection is a range of one row. So the ordinary way to sort a
column -- click a cell in it, press A-Z -- did nothing, with no message, and the
two sort buttons looked exactly like the ten other toolbar buttons that turned
out to be genuinely unwired.

The options, none of them obviously right:

| | *What changes* |
|---|---|
| Leave it | Sorting requires selecting the rows first. Two buttons that do nothing in the common case. |
| Whole column | A-Z sorts every filled row in the column, jumping any blank gap. Two unrelated tables get merged. |
| **The block around the cell (chosen)** | A-Z sorts the run of filled rows containing the cell, stopping at a blank row. |
| Guess the header | As above, but the first row is held back when it "looks like" a heading. |

The block rule is what LibreOffice and Excel do, and a blank row is where anyone
would say one table ends. The header guess was rejected outright: it is right
most of the time and silently scrambles a table the rest of it, and a wrong
sort is not visibly wrong -- the data is still there, in an order nobody chose.
The escape hatch is the behaviour that already existed: select the rows to sort
and the selection is obeyed literally, which is how a header is kept out.

The cost is that sorting the seeded sample sheet descending sinks its "Item"
header to the bottom. That is undoable with one Ctrl+Z, and it is what the user
asked for with a header they did not exclude.
