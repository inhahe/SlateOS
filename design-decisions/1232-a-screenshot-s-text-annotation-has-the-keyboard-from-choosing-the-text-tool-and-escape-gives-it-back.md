## 1232. A screenshot's text annotation has the keyboard from choosing the text tool, and Escape gives it back

**Date:** 2026-10-04
**Lane:** E
**Decided by:** Claude (autonomous)

**In short:** on a captured picture, the keys 1 to 4 choose the drawing
tool and Escape throws the picture away. That made a text label on the
picture impossible to write with a digit in it -- `2nd floor` switched to
the arrow tool at the `2` -- and Escape, pressed to abandon a half-typed
label, threw away the whole screenshot. Now choosing the text tool (key 3,
or its button) opens a box that has the keyboard: every key types into it,
digits included. Escape empties the box; Escape on an empty box gives the
keyboard back, after which 1 to 4 choose tools again and a further Escape
discards the picture as before. A press on the box takes the keyboard back.

### Context

`known-issues/E-the-screenshot-tools-text-annotation-cannot-take-a-digit-and-escape-throws-the-picture-away.md`.
The text being typed was a string appended to and popped from, shown as a
bar reading `Text: a_` only once something had been typed; the tool keys
were matched before it, and Escape discarded the capture unconditionally.

### What was decided

- The text tool's box is the toolkit's field (`guitk::field`) in a strip
  above the status bar, shown whenever the text tool is chosen, with
  textline's editor over the text: the caret keys, Backspace and Delete at
  the caret, Ctrl+A/C/X/V, a press that puts the caret under the pointer.
  Empty, it says "Type the text, then click where it goes", with the caret
  before it.
- **The box has the keyboard from the moment the text tool is chosen**
  (`ScreenshotApp::choose_tool`), so a label can *begin* with a digit.
- **Escape is the box's while it has the keyboard**: a text in it is
  emptied; an empty box gives the keyboard up
  (`annotation_text_focused = false`). Only then do 1-4 and Escape mean
  what they mean on a picture without a text being written.
- Placing a text (a press on the picture) empties the box and leaves it the
  keyboard, for the next label.
- Ctrl+S and Ctrl+Z stay the window's while the box has the keyboard: save,
  and undo the last annotation. The box's undo is not offered.
- Discarding a picture drops the text being typed for it.

### Alternatives considered

- **Give the box the keyboard only once something is typed** (the known
  issue's first suggestion). A digit could still not begin a label, and the
  first key decides whether the box exists, which no one can see.
- **Move the tools off the digits** (letters, or the toolbar alone). The
  digits are documented on the list of keys and in the status bar, and the
  letters are what a label is made of; this would trade one collision for
  another. Choosing tools with the pointer stays possible either way.
- **Escape always discards the picture.** That is what lost captures.

### Where it lives

`apps/screenshot/src/main.rs`: `choose_tool`, `text_box_rect`,
`edit_annotation_text`, `press_text_box`, `handle_key_preview`,
`render_text_box_text`. Tests:
`a_text_annotation_takes_digits_and_escape_abandons_only_the_text`,
`the_text_box_is_the_toolkits_field`.

### How to reverse

To let the digits choose tools while typing, drop the
`annotation_text_focused` branch at the top of `handle_key_preview`'s plain
keys; to have Escape discard from anywhere, drop its arm there. Both are a
few lines, and the tests above name what each changes.
