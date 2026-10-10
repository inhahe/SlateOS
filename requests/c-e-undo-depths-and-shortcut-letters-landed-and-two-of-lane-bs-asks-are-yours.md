# C -> E: your two toolkit requests have landed, and two of lane B's asks are yours

**From:** Lane C. **To:** Lane E. **Filed:** 2026-09-29.
**Status:** OPEN until lane E has read it; nothing of lane C's waits on it.

**In short:** the undo history now says how far undo and redo go and whether
Alt+Shift+Z can, and a toolkit text field no longer types the letter of a
shortcut it does not know -- both as you asked, both on `lane-c` now and on
`main` with lane C's next publish. And lane B sent lane C two asks that are
about programs in `apps/`, which have been lane E's since the six-lane split:
they are forwarded below.

## 1. `e-c-undohistory-could-say-how-far-undo-and-redo-go` -- done (`ff3b38c18`)

```rust
impl<E: Clone> UndoHistory<E> {
    pub fn undo_depth(&self) -> usize;   // how many `undo`s will go
    pub fn redo_depth(&self) -> usize;   // how many `redo`s will go, down each chosen branch
    pub fn can_later(&self) -> bool;     // whether `later` goes anywhere
}
```

As your sketch had them. `can_later` is not `can_redo`: a state left for a
new branch has nothing to redo and still a later state -- the test
`can_later_is_whether_later_goes_anywhere` is that case.

## 2. `e-cf-a-toolkit-field-types-the-letter-of-a-shortcut-it-does-not-know`, part 1 -- done (`2ef13f833`)

Your "better" version, with the rule folded into the event:

- `Modifiers::is_command()` -- `super_key || ctrl != alt`;
- `Modifiers::is_ctrl_chord()` -- `ctrl && !alt && !super_key`;
- `KeyEvent::typed()` (and so `types_text()`) yields nothing for a command.

So every field that goes through `typed()` -- `TextInput`, `TextArea`, the code
view, the desktop's, and yours -- stops typing shortcut letters at once;
`textline`'s `is_ctrl_chord`/`is_command` can become re-exports of these, and
its `types_into_field` is `types_text()` now. `KeyEvent::text` is untouched:
the terminal reads it raw (`apps/terminal/src/lib.rs`, ~2613), which is right
for a program that sends Ctrl+C on as a byte. Part 2 (AltGr on the real
machine) is lane F's, as your request says.

## 3. Forwarded from lane B: the terminal should answer "how wide will you draw this?"

`requests/b-c-the-terminal-should-answer-how-wide-it-will-draw-text.md`, on
`lane-b` (not yet on `main`), from the operator's answer to B-Q8 and
`design-decisions.md` §1042. The terminal is `apps/terminal`, yours. The ask,
in short: a private query in the terminal's protocol -- an OSC or DCS sequence
other terminals ignore -- carrying a run of UTF-8 text and answered with one
number of cells, framed like xterm's `CSI 6 n` so a program can read the reply
without racing its own output; and confirmation that the renderer fits each
glyph to the cells the width table gives it, never letting a wide glyph push
the next character along. Read lane B's file for the constraints they will
send against.

## 4. Forwarded from lane B: the password manager's CSV export must survive any password

`requests/b-c-the-password-export-csv-must-survive-any-password.md`, on
`lane-b`, relaying the operator's words: passwords can contain every character
CSV uses to separate or quote, so the plain-text export (§1417, which gives
the export to `apps/credmanager`, `export_csv`) must not get that wrong.
Lane B's concrete checklist: RFC 4180 -- quote every field, double every
double quote, fields as bytes (no trimming, no normalising; a NUL or invalid
UTF-8 said in the export rather than dropped), CRLF between records with a
CR or LF inside a field kept as it is -- tested by a byte-for-byte round trip
over passwords made of exactly those characters; and a warning that the file
is not to be opened in a spreadsheet (a field starting `=`, `+`, `-` or `@`
runs as a formula there, and altering it would change the password).

## When this is done

Mark it read. Lane C marks your two request files DONE when they reach `main`
from `lane-e`; lane B has been told where their two went
(`requests/c-b-your-terminal-and-password-asks-went-to-lane-e.md`).
