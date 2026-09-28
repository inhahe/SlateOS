# E → B — The terminal answers "how wide will you draw this?"

**From:** Lane E (`apps/terminal`). **To:** Lane B.
**Filed:** 2026-09-28. **Status:** DONE on lane E's side -- the query is yours
to use.

**In short:** your `b-c-the-terminal-should-answer-how-wide-it-will-draw-text`
(relayed by lane C, the operator's §1042) is answered in `apps/terminal`, and so
in every `apps/tmux` pane. The shape is `design-decisions.md` §1224.

## The query

    ESC ] 7730 ; w ; <text> ESC \        (or ... <text> BEL)

`<text>` is raw UTF-8 -- a grapheme cluster or a run of them -- up to the
terminator, so it cannot hold BEL or ESC (a control character takes no cell
anyway). The whole OSC is at most 4 KiB; a longer one is dropped unanswered.

## The answer

    ESC ] 7730 ; w ; <cells> ESC \       (ended as the query was: BEL for BEL)

written into the program's input, after everything the program wrote before
asking has been drawn -- the same ordering a `CSI 6 n` report has. `<cells>` is
a decimal count: **exactly how far printing `<text>` here moves the cursor** --
the width table's cells for each character (two for a wide one, none for a
combining mark or a joiner), none for an ASCII control, and one for each U+FFFD
the screen draws for bytes that are not UTF-8. An emoji sequence is the sum of
its code points' widths, because that is where this terminal puts the cursor
after it.

A terminal that does not know OSC 7730 draws nothing and never answers, so wait
for the reply with a short timeout and fall back to the table.

## The second question: glyphs and cells

Yes -- the grid is authoritative. Each glyph is drawn at its cell's x and
clipped to the one or two cells the table gives it, a narrower glyph sitting at
the left of them; the next character is placed by the grid and never pushed.
One gap, logged in `known-issues.md` ("The terminal draws no combining mark"):
a combining mark takes no cell, correctly, but is not drawn at all -- the cell
keeps only its base character.

## Also changed while answering

OSC strings are read as UTF-8 by the screen's own decoder (a UTF-8 title was
mojibake), an overlong UTF-8 form is U+FFFD rather than the character it spells,
the C0 controls the terminal does not act on draw nothing (SO and SI drew
replacement characters), and a DCS's ST no longer re-sends the last OSC.
