## 1480. A rich input is a string and runs of formatting, and typing carries on in the format before the caret

**Date:** 2026-10-05 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the toolkit has a text field whose text can be bold, italic,
underlined, struck through, coloured and sized -- for a message body, a note,
a description -- with Ctrl+B, Ctrl+I and Ctrl+U and an optional toolbar that
shows what the selection is. It behaves as a word processor does: text typed
goes on in the format of the text before the caret, and a switch pressed with
nothing selected applies to what is typed next. A paste keeps its formatting
when it is this program's own copy; anything else pastes as plain text.

**Where:** `gui/toolkit/src/richinput.rs` and `richinput/` (`doc`, `layout`,
`toolbar`). `roadmap-detailed.md` §3.5, *Input Fields*: "Rich input with
formatting and image paste (optional formatting toolbar)".

| Choice | Instead of | For | Against |
|---|---|---|---|
| **A document is a string and runs that tile it**, neighbours alike merged | a tree of elements, as HTML is | Every byte has one format; a slice, a replacement and an undo are each one operation on the same value, and two documents that read the same are equal. | Paragraph attributes -- alignment, lists -- will need a layer of their own beside the runs. |
| **Typing carries on in the format before the caret; a switch with nothing selected is for what is typed next**, until the caret moves | typing always plain, or in a "current format" kept apart from the text | The rule every word processor has taught its users: put the caret after a bold word and type, and the new text is bold. | A user who wants plain text after bold presses Ctrl+B first, as everywhere. |
| **A switch over a selection turns on where any of it lacks the format, off where all of it has it** | toggling each run separately | One press makes a mixed selection uniform, and the toolbar's pressed state ("all of it") says which way the next press goes. | -- |
| **A paste is formatted only when it is this program's own last copy**; otherwise plain, in the format at the caret | pasting formatting from anywhere | The clipboard carries text only (`TD-C-NOTHING-CAN-ACTUALLY-COPY-AND-PASTE-BETWEEN-PROGRAMS`); the formatting of a copy made here is kept beside it and matched by its text, so it comes back exactly where it can, and nothing pretends where it cannot. *Amended 2026-10-06 (§1486): matched by the clipboard's generation, which every copy changes, rather than by its text -- the same text copied by another field is another copy -- and a copy holding pictures comes back with them.* | A rich copy between two programs pastes plain until the clipboard carries formats. |
| **A colour and a size are the program's to choose** (`set_color`, `set_size`); the toolbar has the switches, a size smaller and larger, and clearing | a colour picker and a size list in the toolbar | The program has the colour picker, the theme's colours and the sizes that suit its document; the toolbar stays small and optional. | A program that wants colours on the toolbar puts them there itself. |
| **Italic is kept and saved, and drawn upright** until the font cache draws a slanted face | refusing italic | A document's italic is not lost while the drawing waits (`requests/c-f-text-at-any-weight-and-in-italic.md`). | Italic text does not look italic yet. |
| **Runs are laid out in written order** (`known-issues-resolved/TD-C-A-RICH-INPUT-LAYS-ITS-RUNS-OUT-LEFT-TO-RIGHT.md`) -- *superseded 2026-10-06: a line is laid out by the bidirectional algorithm over its paragraph (`osfont::bidi`'s levels; pieces cut at a change of direction, placed in screen order; the caret drawn on the side of a change its affinity names; the arrows step on the screen)* | the bidirectional algorithm across fonts | Text in one direction -- the case its users have -- is laid out right now; the bidi pass needs the shaper's levels across fonts. | A line mixing directions shows its runs in written order. |
