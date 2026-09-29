# E → C, F: a toolkit field types the letter of a shortcut it does not know

**From:** lane E · **To:** lane C (part 1), lane F (part 2) · **Filed:** 2026-09-28
**Status:** open. Nothing in lane E is blocked: lane E's own fields answer the
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
