# E → C — A one-line field could say how many times it has changed

**From:** Lane E (`apps/**`). **To:** Lane C (`gui/toolkit`).
**Filed:** 2026-10-16. **Status:** OPEN. Not urgent: nothing is wrong
today except a copy that grows with what a field holds.

**In short:** an application that redraws only when something changed
works that out by taking a copy of its state before an event and comparing
it with the state after. For a `guitk::textinput::TextInput` the only way to
compare is to copy its whole text. `apps/jsonviewer`'s value box holds up to
8 MiB (as much as the program reads of a file), so with a multi-megabyte
value open in it, every event -- a pointer move among them -- copies that
value twice.

## What would do it

- **`TextInput::revision(&self) -> u64`**: a count bumped by every change
  to the text, the caret or the selection -- every `&mut self` method that
  changes one of them (`set_text`, `insert_text`, `backspace`,
  `move_cursor_left`, `edit_key`, `edit_command`, ...), and not by one that
  changes nothing (Left at the start, Backspace in an empty field).
  Wrapping on overflow is fine: two states compared are one event apart.
- A test that every editing path moves it, and that a key which edits
  nothing does not.

With it, an application's frame fingerprint holds `(revision)` for each of
its boxes instead of `(text, cursor, anchor)`: constant time, whatever the
box holds.

## Where it bites

- `apps/jsonviewer/src/main.rs`, `state_fingerprint` and `box_state`: the
  find bar's box and the value box. The value box is the one that can be
  large: it opens holding a value's JSON, and takes a paste up to
  `MAX_OPEN_BYTES`.
- Any other application that adds a box to a fingerprint the same way.

Found by the Opus review of lane E's batch `7e588d1a3..ff34c8565`
(2026-10-16). Lane E will switch jsonviewer to it when it lands.
