# E → C: a `TextInput` that takes its own keys

**From:** lane E · **To:** lane C · **Filed:** 2026-09-25
**Status:** open — nothing in lane E is blocked. The copies are gone: since
2026-09-26 the seven applications share one, the lane E crate `apps/textline`
(below), which goes when this lands.

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

**The multi-line half, meanwhile (2026-09-25):** a field of many lines --
caret by character and by line, keeping its column across Up and Down,
selection, typing over it, Enter as a line break -- was `apps/regextester`'s
own `TextArea`, and `apps/email`'s message body needed it too. Rather than a
second copy of a few hundred lines, it is now the lane E crate
`apps/textarea`, with its key table as `TextArea::apply_key`. If the toolkit
grows a multi-line field, the two applications move onto it and the crate
goes.

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

## Meanwhile: one copy, not seven (2026-09-26)

A seventh copy had appeared (`apps/email`, for the To, Cc and Subject
fields), and the copies had drifted into four versions: two reported whether
a key was the field's and five did not, and five deleted the selection when a
key typed nothing -- Tab or Enter reaching the field would destroy text it did
not replace. So the table is now one lane E crate, `apps/textline`, the way
the multi-line half became `apps/textarea`:

```rust
pub fn apply_key(input: &mut TextInput, key: &KeyEvent, capacity: usize,
                 clipboard: &str, font_size: f32) -> LineEdit
pub struct LineEdit { pub handled: bool, pub copied: Option<String> }
```

-- the signature asked for above, less the clipboard the field would hold
itself. All seven applications call it; their tests pass unchanged, and the
mutation rows that swept their copies moved to `apps/textline/mutate.py`.

## When it lands

Lane E moves the seven applications onto `TextInput`'s own method and deletes
`apps/textline`.
