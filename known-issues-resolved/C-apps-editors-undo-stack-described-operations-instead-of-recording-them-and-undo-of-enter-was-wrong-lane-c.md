## `apps/editor`'s undo stack described operations instead of recording them, and undo of Enter was wrong (lane C)

**Status: FIXED 2026-08-16** (lane C). Found while fixing the byte-vs-character
bug in the entry above; the two share a cause.

`EditAction` was a four-variant enum naming the operation that had been
performed — `Insert`/`Delete` with a line, a column and the text, and
`InsertLine`/`DeleteLine` with a line and the text. `undo` and `redo` then
reconstructed the inverse from the description. That model requires the
description to stay in agreement with what the operation actually did, and
nothing enforced the agreement, so it drifted in three places at once:

- **Undo of Enter deleted an unrelated character.** Splitting a line pushed
  `Insert { text: "
" }`, and `undo` reverted it by removing one byte at `col`
  *from that line* — but the split had already moved the tail into a new `lines`
  entry, so no line contained the newline. In practice the removal was
  out of bounds and did nothing, leaving the document permanently split.
  Reproduce (before the fix): type `abcd`, put the caret between `b` and `c`,
  press Enter, press Ctrl+Z — expected `abcd` on one line, actual two lines.
- **Enter's auto-indent was not recorded at all.** The new line was prefixed
  with the previous line's leading whitespace and nothing on the stack could
  take those bytes back.
- **`InsertLine` was matched by `undo` and by `redo` and constructed by
  nothing** — dead in the sense that matters, since the operation it existed to
  describe was recorded as something else. (`DeleteLine` *was* constructed, by
  the two join paths.)
- **Only `insert_char` cleared the redo stack.** A backspace after an undo left
  a redo entry describing an edit against a buffer that had since moved on.

**The fix was to stop describing edits and start recording them.** `EditAction`
is now a struct holding the *text of the affected lines before* and *after* the
edit, plus the caret position on each side; undo is `after → before`, redo is
`before → after`, both a single `Vec::splice`. Call sites go through
`Document::record_edit(line, before_count, |doc| …)`, which snapshots, runs the
closure, derives the after-count from how the buffer's total length changed —
a fact, not a claim — and records, invalidates the syntax memo and clears the
redo stack in one place. An edit can no longer misdescribe itself, because it
does not describe itself. Cost is two copies of each touched line per undo
entry; for a 1000-entry stack of single-character edits that is on the order of
100 KiB, which is the right trade for a stack that cannot disagree with the
buffer.

Seven tests in `main.rs`'s `undo_tests`. The load-bearing one is
`every_edit_operation_round_trips_one_step_at_a_time`: it applies all ten
editing operations to a document containing two-byte, three-byte and four-byte
characters plus indentation, then undoes and redoes one step at a time,
asserting against a snapshot taken *between* every pair of steps — an undo stack
can arrive back at the original text while having been wrong at every
intermediate point, and the intermediate points are what the user looks at.
104 tests pass, clippy clean.
