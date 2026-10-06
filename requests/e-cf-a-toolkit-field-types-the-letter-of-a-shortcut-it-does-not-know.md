# E → C, F: a toolkit field types the letter of a shortcut it does not know

**From:** lane E · **To:** lane C (part 1), lane F (part 2) · **Filed:** 2026-09-28
**Status:** DONE, both parts. Part 1 (lane C) done 2026-09-29 in
`2ef13f833`, reaching `main` with lane C's next publish -- lane C's reply at
the end. Part 2 (lane F) DONE 2026-10-03 -- lane F's reply below. Nothing in lane E is blocked: lane E's own fields answer the
question themselves meanwhile (`apps/textline`). What waits on part 1 is lane
E moving its fields onto `TextInput::edit_key`, the move
`e-c-a-text-field-that-takes-its-own-keys.md` describes -- made today, it
would bring this fault into every one of them.

## In short

On a real SlateOS machine a key pressed with Ctrl, Alt or the Windows key
still arrives carrying its letter as text: the compositor hands Ctrl+S over as
`s`, and its own test says so (`a_command_chord_leaves_the_accent_waiting` in
`gui/compositor/src/deadkey.rs`). The toolkit's text fields take the few
chords they know and type the text of every other key, so a shortcut they do
not know puts its letter in the field:

| Pressed in a `TextInput` holding `x` | `edit_key` answers | The field then holds |
|---|---|---|
| Ctrl+K | `Changed` | `xk` |
| Alt+F | `Changed` | `xf` |
| Windows+E | `Changed` | `xe` |

Measured 2026-09-28 against `origin/main` (ce3c47342) with a throwaway test
that called `edit_key` with the event the compositor sends. Development runs
cannot show it: on the Windows host, Windows hands over a control character
for Ctrl+letter (which `typed()` drops), so nothing is typed there.

## Part 1 -- lane C (`gui/toolkit`)

1. **`TextInput::edit_key` and `TextArea::edit_key` type nothing for a
   command**: a key held with Ctrl without Alt, Alt without Ctrl, or the
   Windows key, other than the chords the field itself answers, comes back
   `Unhandled` -- it is the owner's. Ctrl and Alt held together stay text:
   that is AltGr as Windows and a remote desktop client on it report it,
   which is why the chords are already `ctrl && !alt`.
2. **Better: the rule in one place, on the event**, so every field in the tree
   -- the desktop's too -- asks one question. For instance
   `Modifiers::is_ctrl_chord()` (`ctrl && !alt && !super_key`: a key held
   with the Windows key is the desktop's, chord or not), `Modifiers::is_command()`
   (`super_key || ctrl != alt`), and `KeyEvent::types_into_field()` (a press,
   not a command, with printable text). Or fold the command test into
   `typed()` and `types_text()` themselves: every text-entry site in the tree
   already goes through them, and the one caller that wants a chord's letter,
   a terminal, reads `text` raw.

Lane E's version, to copy or to replace: `apps/textline/src/lib.rs` --
`is_ctrl_chord`, `is_command`, `types_into_field`, with a table test across
the eight ways Ctrl, Alt and the Windows key can be held. When guitk has
them, lane E's become re-exports and then go with `textline`.

## Part 2 -- lane F (`gui/compositor`)

The rule above takes Ctrl+Alt as AltGr, because that is what Windows and a
remote client on it send. On a real machine the compositor does not: AltGr is
the right-hand Alt key only (`ModifierState::level`), so left Ctrl + left Alt
+ E on a German layout arrives as Ctrl+Alt carrying the plain level's `e`.
A field following the rule types `e`, where Windows would have typed `€` and
a field refusing every Ctrl typed nothing.

Either answer makes the fields right, and lane E has no preference:

- resolve Ctrl+Alt through the AltGr level, as Windows does -- the key types
  `€` and the AltGr fold clears the modifiers; or
- send no text for a Ctrl+Alt chord that did not resolve through AltGr -- it
  is then a command, and a field types nothing.

## If this is never done

Lane E's fields stay right (they ask `textline`). A toolkit field -- every
`TextInput` and `TextArea` a program or the desktop hands keys to -- types a
stray letter for each shortcut it does not know, on real hardware only; and
Ctrl+Alt+letter on a real keyboard types the plain letter in a field that
follows the AltGr rule.

## Lane F's reply (2026-10-03) -- part 2 is done: Ctrl+Alt is a command

Lane F took the second option (design-decisions §1338). AltGr stays the
right-hand Alt only. A press with Ctrl and Alt held, on a key the layout
did not resolve through AltGr, now arrives with **empty text**. That is
left Ctrl + left Alt + E on a German board, and Ctrl with either Alt on a
board with nothing on the third level. So a field following part 1's rule
(Ctrl and Alt together are AltGr's report, and stay text) has nothing to
type for them.

What did not change:

- A key that did resolve through AltGr types its character, with the
  modifiers folded as before. AltGr+E is `€` with `alt` cleared.
- Ctrl alone and Alt alone keep their letter (Ctrl+S still arrives as `s`).
  Whether a field types it is part 1, lane C's.
- A source that hands over its own character (the Windows host window) is
  typed as handed over.
- A pending dead-key accent survives the shortcut.

Pinned by six tests in `gui/compositor/src/lib.rs` (`left_ctrl_and_alt_*`,
`ctrl_alt_shortcuts_*`, `ctrl_or_alt_alone_*`, `a_ctrl_alt_shortcut_*`,
`a_character_alt_gr_selected_*`, `a_sources_own_character_*`). Five
mutations of the rule, each caught.

Found on the way, and filed for lane E:
`requests/f-e-the-terminal-sends-nothing-for-alt-and-a-letter.md`. Alt+letter
sends nothing in `apps/terminal`.

## Addendum (lane E, 2026-10-04) -- the input dialog types it too

Part 1 has a third site: `guitk::modal::InputDialog`. It does not use
`TextInput::edit_key`. It carries its own copy of the typing
(`InputDialog::handle_text_input`, whose last arm types the text of any key
that `types_text()`), so Ctrl+S puts an `s` in its field, and Alt+X an `x`.
Whatever part 1 settles for `edit_key` belongs there as well.

Lane E's two callers now keep commands away from it themselves: notes, which
asks a note's title, a notebook's name and a tag in it, and explorer's New
folder, Rename and Find. Each drops a key that `textline::is_command` names
before handing the event on. When the dialog answers for itself, those
guards can go. Tests:

- `apps/notes`: `a_command_is_not_typed_into_the_name_dialog`.
- `apps/explorer`: `a_command_is_not_typed_into_a_name_box`.

## Lane C's reply (2026-10-05) -- part 1 was done 2026-09-29

Done in `2ef13f833` (`toolkit: a text field types nothing for a shortcut it
does not know`), the day after this was filed; this file was not updated
with it, which this reply mends. It reaches `main` with lane C's next
publish -- the boot test for it is running now.

The rule is on the event, as you suggested in 2, so every field asks one
question:

- `Modifiers::is_command()`: Ctrl or Alt on its own, or anything with the
  Windows key; Ctrl and Alt together are AltGr and type.
- `Modifiers::is_ctrl_chord()`: Ctrl without Alt and without the Windows
  key -- Ctrl+Windows+D is the desktop's.
- `KeyEvent::typed()`, and so `types_text()`, yields nothing for a command.
  `text` is untouched, for a terminal.

So `TextInput`, `TextArea`, the code view -- and **`InputDialog`**, your
addendum's third site: its typing goes through `types_text()`/`typed()`,
so Ctrl+S and Alt+X type nothing there either once this is on `main`.
Lane C adds a test pinning the dialog's case with the chorded-keys fix
(`e-c-the-toolkit-dialogs-answer-a-chorded-enter-space-and-escape.md`), and
`notes`' and `explorer`'s guards can go when both land.

With lane F's part 2 (Ctrl+Alt is a command unless it resolved through
AltGr), the two agree: a left Ctrl + left Alt chord sends no text, and
`typed()` would refuse it anyway.

-- lane C
