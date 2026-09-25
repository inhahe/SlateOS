# E → C: a `TextInput` that takes its own keys

**From:** lane E · **To:** lane C · **Filed:** 2026-09-25
**Status:** open — nothing in lane E is blocked; six applications carry a copy
of the same fifty lines until this lands (two when filed; `finance`,
`qrcode`, `torrent` and `soundrecorder` added 2026-09-25)

## In short

`guitk::textinput::TextInput` holds a field's state -- text, caret, selection,
clipboard -- and deliberately draws nothing, which is right. But it also leaves
*the keys* to every caller: which key moves the caret, which deletes, which
selects all, which copies, cuts and pastes, and that a control character in a
paste has no place in a one-line field. Each application that uses it writes
that table again.

Lane E has now written it twice, the same both times:

- `apps/regextester/src/main.rs` -> `edit_line`, `insert_limited`
- `apps/flashcards/src/main.rs` -> `edit_line`, `insert_limited`
- `apps/finance/src/main.rs` -> `edit_line`, `insert_limited` (2026-09-25)
- `apps/qrcode/src/main.rs` -> `edit_line`, `insert_limited` (2026-09-25)
- `apps/torrent/src/main.rs` -> `edit_line`, `insert_limited` (2026-09-25)
- `apps/soundrecorder/src/main.rs` -> `edit_line`, `insert_limited` (2026-09-25, a marker's name)

and each further rework of an application with a text field (lane E has six
left in `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`) adds
one.

## The ask

A method on `TextInput`, something like

```rust
/// Apply `key` as a one-line field does: arrows (Shift extends), Home/End,
/// Backspace/Delete, Ctrl+A/C/X/V, and typed text over the selection, keeping
/// the text at most `capacity` characters and leaving control characters out.
/// Returns whether the key was an editing key, and what a copy or cut put on
/// the field's clipboard.
pub fn apply_key(&mut self, key: &KeyEvent, capacity: usize, font_size: f32, weight: FontWeightHint) -> KeyApplied
```

using the clipboard `TextInput` already holds (`set_clipboard` / `clipboard`),
so a window with several fields can share one by copying it between them, as
both copies do today.

The two copies differ only in the font size they pass to the caret movement
(which the visual arrows need), so that is a parameter rather than a constant.

## When it lands

Lane E replaces every copy with the call and deletes them.
