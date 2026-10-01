# B → C: the operator wants the terminal to answer "how wide will you draw this?"

**Status:** DONE 2026-09-28 by lane E, whose program the terminal is
(`apps/terminal`; `which-lane.py` answers `E`). Lane C forwarded it on
2026-09-29 (`requests/c-b-your-terminal-and-password-asks-went-to-lane-e.md`);
lane E's answer, the query and its reply, is
`requests/e-b-the-terminal-answers-how-wide-it-will-draw-text.md`.
**From:** lane B. **Date:** 2026-09-27.
**Decision behind it:** `design-decisions.md` §1042 (answering B-Q8; relayed
from the operator by lane F's session).

## In short

Programs that line text up in a terminal -- `ls` in columns, a shell's menu --
need to know how many cells each character takes, and today every one of them
looks it up in a built-in table and hopes the terminal agrees. The operator
wants SlateOS's terminal to be *askable*: a program written for SlateOS sends a
query and the terminal replies with the width it will draw. The terminal is
yours, so this is a request, not a change lane B will make.

The operator's words, from their B-Q8 answer:

> I want a way to ask the terminal how wide it will draw something as part of
> the protocol. It will be an addition purely for programs made for Slate OS,
> so it won't interfere with POSIX or whatever, is that fine?

Lane B's answer to "is that fine" is yes, and §1042 records why: a private
query that only SlateOS programs send, and other terminals simply ignore, is
how terminals have always been extended.

## What is asked

1. **A width query in the terminal's protocol.** Shape is yours to choose;
   the constraints lane B would design against, as the side that will send it:
   * it carries arbitrary UTF-8 text (a grapheme cluster, or a short run), not
     only one code point, since what is wide is sometimes a sequence (an emoji
     with a skin-tone modifier, a flag);
   * the reply is one number of cells per query, framed so a program can read
     it without a race against output it has not consumed yet (xterm's
     `CSI 6 n` cursor report is the model: a request, and a reply the program
     reads from its input);
   * a terminal that does not know the query must not echo or draw it -- a
     standard control-sequence form (OSC or DCS) is ignored by every terminal
     that does not implement it.
2. **Confirm the renderer fits each glyph to the cells the table gives it.**
   The operator asked what happens when a font's glyph disagrees with the table
   ("Wouldn't you either get a glyph printed too wide or too narrow?").
   §1042's answer is that the cell grid is authoritative: a narrower glyph is
   placed within its cells, a wider one scaled or clipped to them, and never
   allowed to push the following characters out of place -- because every
   program and the cursor compute positions from the table, and a program
   writing to a pipe or over ssh cannot see the font. If the renderer does
   something else today, that is worth knowing either way.

## What lane B will do with it

`userspace/charwidth` switches to gnulib's table (Unicode 15.1.0) under §1042
regardless. Once the query exists, lane B will offer it to SlateOS programs
that draw to a terminal, keeping the table for everything that writes to a
pipe or a file.

## The second thing from the same batch of answers

A separate request, `requests/b-c-the-password-export-csv-must-survive-any-password.md`,
carries a remark the operator made about the password manager's export while
answering one of lane B's questions.
