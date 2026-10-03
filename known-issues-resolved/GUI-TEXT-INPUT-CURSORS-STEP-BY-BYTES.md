## GUI-TEXT-INPUT-CURSORS-STEP-BY-BYTES

**Status: FIXED 2026-08-16**, found while auditing the widgets that hold a text
cursor for `TD-GUI-WIDGET-CARETS-ARE-NOT-BIDIRECTIONAL`.

**What.** `gui/toolkit/src/widget.rs` (`WidgetKind::TextInput`) and
`gui/toolkit/src/modal.rs` (`InputDialog`) both store the cursor as a *byte*
offset — they must, because `String::insert` and `String::remove` index by
bytes — but moved it by **one** on every Left, Right, Backspace and (in
`InputDialog`) every character typed.

**Symptom: a panic, not a misplacement.** `String::insert`/`remove` panic when
the offset is not on a character boundary. So one byte-sized step into a
multi-byte character armed the crash and the *next* edit fired it. Typing
`café` into an `InputDialog` and pressing Backspace was enough: the cursor
landed at byte 4, in the middle of the two-byte `é`, and `remove(4)` aborted.
Every non-ASCII input — an accented file name, any Cyrillic, Greek, Hebrew,
Arabic, CJK or emoji text — was a crash one keystroke away. `InputDialog`'s
insert path was worse still: it advanced by one per character typed, so the
cursor drifted further from a boundary with every non-ASCII letter.

This is the same class of bug as the caret entry above (a byte offset treated
as though it counted characters) but strictly more serious: the caret bug drew
in the wrong place, this one took the process down.

**Fix.** Every move now steps by the width in bytes of the character being
crossed, obtained from the string itself:
`value.get(..cursor).and_then(|before| before.chars().next_back())` for a
leftward step and `value.get(cursor..).and_then(|after| after.chars().next())`
for a rightward one. Both return `None` at the ends, which replaces the
`> 0` / `< len` guards and removes the underflow with them. `Delete` needs no
arithmetic — `String::remove` takes the whole character at the offset — only
the guard that the offset is inside the string.

**Tests.** `widget::tests::a_text_input_survives_a_multi_byte_character` and
`modal::tests::a_multi_byte_character_can_be_typed_moved_over_and_deleted`,
which type `café`, step over the `é` in both directions, and delete it from
both sides. Both panicked before the fix.

**The rest of `apps/**` and `gui/**` was then audited, and is clean.** Every
call site that edits a `String` at a cursor was checked:

| Site | Verdict |
|---|---|
| `apps/editor` | **was broken**, fixed here |
| `gui/toolkit` `TextInput`, `InputDialog` | **were broken**, fixed here |
| `apps/markdowneditor` | already correct — has a `clamp_col` helper and steps by `len_utf8` throughout |
| `apps/paint` (text tool) | already correct — scans for the adjacent character |
| `apps/launcher`, `gui/desktop/launcher.rs` | already correct — `char_indices` |
| `gui/desktop/run_dialog.rs` | already correct — scans back to a boundary |
| `apps/jsonviewer` | correct by a different route: its cursor counts *characters* and is converted with `char_to_byte_pos` at every use |
| `apps/unitconverter` | correct **only by invariant** — the field's input filter accepts nothing but ASCII digits, `.`, `-`, `e`, `E`, so one character is one byte. A comment now records that widening the filter without switching to `len_utf8` reintroduces the panic. |

Everything else matching `cursor ± 1` in `apps/**` is a *grid* cursor (a row or
column in a game board or a table), not a string index.
