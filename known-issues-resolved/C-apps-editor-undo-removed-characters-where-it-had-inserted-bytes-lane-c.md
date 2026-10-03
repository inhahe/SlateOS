## `apps/editor` undo removed characters where it had inserted bytes (lane C)

**Status: FIXED 2026-08-16** (lane C). Found while wiring up syntax
highlighting; pre-existing, and independent of that work.

`EditAction::Insert { line, col, text }` records the *bytes* inserted at a byte
offset. Undo reverted it with `for _ in 0..text.len() { current.remove(col) }` —
but `String::remove` removes one **character**, so undoing an inserted `e`-acute
(two bytes) deleted two characters: the accented one and whatever followed it.
Redo of a `Delete` had the identical loop and the identical bug. On an ASCII
document the two counts coincide and nothing looks wrong, which is why it
survived; the first non-ASCII character in a file made undo silently eat a
neighbour, and if the offset landed mid-character `remove` panicked outright.

Fixed by `remove_bytes(&mut String, at, len)`, which checks both ends are char
boundaries and the range is in bounds before `replace_range(at..end, "")`, and
does nothing if not — a no-op undo is recoverable, a panic in an editor is not.
Covered by `undoing_a_multi_byte_insertion_removes_only_that_character`.
