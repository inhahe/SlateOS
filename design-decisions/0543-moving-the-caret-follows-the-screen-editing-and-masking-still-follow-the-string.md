## 543. Moving the caret follows the screen; editing and masking still follow the string

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Claude (operator-approved scope) — §541 is the operator's decision; which *other* keys follow it is mine

**In short:** §541 says the Left and Right arrow keys move the caret by what is
on the screen rather than by position in the sentence. It does not say what
Backspace does, or what a password field does, and both have to be decided
before the change can be written. **Backspace and Delete keep following the
string** — they remove the character before or at the caret *in reading order*,
wherever that character happens to be drawn. **A password field keeps moving by
the string too**, and is the only field that does. So on a line mixing English
and Hebrew, pressing Left and then Backspace can delete a character other than
the one immediately left of the caret on screen.

**Glossary:** *logical* order is the order characters are stored and read in;
*visual* order is the order they are painted in. On a line that mixes a
left-to-right script with a right-to-left one they differ, and that is the only
case where any of this matters.

### Why editing stays logical

An arrow key names a **direction on a screen** — that is the whole argument
§541 turned on. Backspace does not. It names *the character you just typed*,
and that character is the previous one in the string no matter which way its
script runs. A reader of Hebrew pressing Backspace after typing a Hebrew letter
means that letter; a Backspace that deleted whatever was painted to the left
would delete the *next* letter of the word they are in the middle of writing.

So the two keys genuinely disagree, and the disagreement is not an
inconsistency to be tidied away — it is each key doing what its own name says.
The visible cost is that Left-then-Backspace is not always "delete what I just
moved past". That is accepted: it is the rarer operation, and the alternative
makes typing a right-to-left word impossible.

Home, End and word-motion stay logical for the reason §541 already gives:
they name positions in the sentence, not directions on the screen.

### Why a password field is the exception

What a password field draws is a row of identical asterisks, in string order,
whatever was typed. There is no visual order to follow — the mask has erased
it. Moving by the layout of the *hidden* text would therefore scatter the caret
among indistinguishable marks with nothing on screen to explain the jumps.

Worse, the jumps are themselves a disclosure: the pattern of where the caret
skips is a readout of where the secret changes direction, which narrows the
script it is written in to anyone watching the screen. Preventing exactly that
is what masking is for. A field whose contents are not drawn cannot take a rule
about how contents are drawn.

The same reasoning fixed a second defect found next door: the mask was built
with `"*".repeat(text.len())` — the UTF-8 *byte* count — so an eight-character
non-Latin password drew sixteen marks and leaked its encoding. It counts
characters now.

### Rejected

- **Editing follows the screen too.** Self-consistent, and it would make
  Left-then-Backspace always undo the Left. Rejected because it makes writing
  a right-to-left word delete the wrong letter, which is not a corner case for
  the users the feature exists for — it is every keystroke.
- **The password field follows the screen like everything else.** One fewer
  rule. Rejected on the disclosure argument above; a uniform rule that leaks
  the shape of a secret is not simpler in any sense that matters.

### Where this landed (superseding §541's shorter list)

§541 named two widgets. There were five, and finding the other three mattered:
a desktop with some fields switched and some not moves its caret by two rules
depending on which box has focus, which §541 records as *worse* than not
switching at all.

| Field | Arrows | Caret drawn by |
|---|---|---|
| `guitk::widget::TextInput` | visual | **nothing** — see `known-issues.md` → `TD-C-TWO-TOOLKIT-TEXT-FIELDS-DRAW-NO-CARET-AT-ALL` |
| `guitk::modal::InputDialog` | visual (logical when `password_mode`) | **nothing** — same entry |
| `guitk::pathbar::PathBar` | visual | `text::caret_x` |
| `desktop::launcher` | visual | `text::caret_x` |
| `desktop::run_dialog::TextInput` | visual | `text::caret_x` |

Moving correctly is only half of it. The caret must also be *drawn* where it
moved to, and the prefix-width measurement every one of these used is right
only while a line runs in one direction. Two of the three that draw a caret
were drawing it wrongly, and one of those (`run_dialog`) sliced its text at the
caret's raw byte offset inside `render`, so a drifted cursor aborted the whole
desktop shell while merely *painting* the dialog. Both are fixed and both now
have a test that watches the drawn caret rather than the cursor value —
the only two such tests in the tree.

`apps/editor` remains outside all of this, per §541.
