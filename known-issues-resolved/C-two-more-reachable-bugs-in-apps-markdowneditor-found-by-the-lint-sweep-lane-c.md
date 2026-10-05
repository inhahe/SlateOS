## Two more reachable bugs in `apps/markdowneditor`, found by the lint sweep (lane C)

**Status: FIXED 2026-08-16** (lane C), commit `2a5c23082`. Neither was caught
by any existing test; both now have one.

**1. Undo of a heading duplicated the line instead of removing the prefix.**
`insert_heading` inserts `"## "` at column 0, then recorded the undo as a
`Delete` of the *old* line text. Undo therefore re-inserted the whole line in
front of itself: pressing the H2 toolbar button on `Title` and then Ctrl+Z
gave `Title## Title`, not `Title`. Proved reachable by reverting the fix and
watching the new test fail with exactly that string. Fixed by recording what
actually happened — an `Insert` of the prefix at column 0.

**2. Rendering past the end of a shrunken document indexed off the end.**
`render_editor`'s visible-line loop indexed `doc.lines[scroll_line + i]` and
broke on reaching the end, so the bound was stated twice and the two could
drift: a `scroll_line` left over from before a document shrank under the
viewport indexed past the end on the first iteration, before the break could
run. Replaced with `.skip(scroll_line).take(visible_lines)`, which states the
bound once and draws nothing rather than panicking.
