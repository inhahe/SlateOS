## 878. The multi-line text field: a wrap boundary is the cursor's affinity, the vertical scroll is state, undo goes by the word

**Date:** 2026-09-25 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The toolkit had a text field for one line and nothing for more,
so every program that needed a box of text -- a note, a message -- wrote its
own or did without: the notes app can only add to the end of a note. There is
now one, `guitk::textarea`, and the desktop's notes are written with it. Five
choices in it are worth recording, because each has a reasonable alternative.

### The calls

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| A wrapped line ends at the offset the next one starts at. Which line is a caret there on? | the cursor's own `Affinity`: `Upstream` the line above (where End puts it), `Downstream` the line below | a separate "at end of line" flag, or no end-of-line stop | the cursor already carries one bit for "one offset, two places on the screen" -- the two sides of a change of direction -- and this is the same question. Every visual line gets its own start and end stops, which is predictable and needs no special case in Left and Right. |
| Is the view's position state? | the vertical offset is stored, and clamped on every read; the horizontal one is worked out from the caret each time | derive both from the caret, as §546 does for one line | a box of several lines is read as well as written: the wheel moves the view without moving the caret, which a derived offset cannot express. Sideways there is nothing to read that the caret cannot reach, so §546's rule stands there. |
| How much does one undo take back? | a word of typing, or a run of Backspaces or Deletes; a newline, a paste or a replaced selection is a step of its own; moving the caret ends a step | one step per keystroke, or whole seconds of typing | per keystroke is an undo nobody uses twice; by time, a slow typist and a fast one get different undo. The word is what a reader thinks in. The history keeps edits, not copies of the text, and reaches back 500 steps. |
| Which keys does the field not take? | Tab, and Ctrl+Enter | Tab indents | a notes box that ate Tab would trap a keyboard user in it; Ctrl+Enter is the usual "send" in a message body. Both come back unhandled for the owner. |
| When is a note saved? | at every change | when it is closed | a note is the thing on a desktop most worth not losing, and the desktop can end with a note open. A keystroke is a change the user made, not a step of a gesture -- the widget layout's rule against a write per drag step does not apply -- and the file is small. |

### What is not done here

- The apps that edit text are lane E's to move onto it
  (`requests/c-e-a-multi-line-text-field-for-the-apps-that-edit-text.md`).
- `WidgetKind::TextArea` in the toolkit's widget tree still holds only a
  value; putting this field behind it is the next step for a program that
  builds its window from that tree.
- A triple click selects the paragraph in the field, but the desktop's event
  loop reports only single and double clicks.
