### [E] The screenshot tool's text annotation cannot take a digit, and Escape there throws the picture away -- 2026-10-03

**Status:** OPEN until the fix -- lane E, 2026-10-04: the text tool's box
has the keyboard from choosing the tool, Escape empties it and then gives the
keyboard back (design-decisions §1232) -- has had a boot test on `main`; then
this moves to `known-issues-resolved/`. Found during the Alt/Windows-key pass;
not caused by it.

**In short:** on a captured picture, the digits 1-4 choose the annotation
tool (rectangle, arrow, text, highlight) whatever is happening, so a text
annotation can never contain a digit: typing `2nd floor` turns the `2` into
the arrow tool, and the text being typed vanishes from the screen with it
(the bar showing it is drawn only while the text tool is chosen). And Escape
on the picture discards the whole capture -- so a user who presses Escape to
abandon the text they are typing loses the screenshot instead.

**Reproduce:** capture anything, press 3 (text), type `a2`. The tool is now
the arrow and the `Text: a_` bar is gone. Press 3 again: the bar is back
reading `a`. Press Escape: the picture is gone, not the text.

**Where.** `apps/screenshot/src/main.rs`, `handle_key_preview`: the
`Key::Num1`..`Key::Num4` arms come before the arm that types into
`annotation_text_input`, and `Key::Escape` calls `discard_current`
unconditionally.

**The proper fix.** While a text annotation is being typed (the text tool
chosen and `annotation_text_input` not empty) the keyboard is the text's:
every typed character goes in, digits included; Escape abandons the text and
leaves the picture; the tool keys wait until the text is placed or
abandoned. With nothing typed, the digits and Escape keep their meaning.
That still cannot start a text with a digit; a tool key that is not a digit
(or a toolbar-only text tool) would. That choice -- which keys choose the
tools -- is the design question, and the fix should record it in
`design-decisions.md`.
