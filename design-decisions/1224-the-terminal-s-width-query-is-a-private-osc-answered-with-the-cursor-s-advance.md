## 1224. The terminal's width query is a private OSC, answered with the cursor's advance

**Date:** 2026-09-28
**Lane:** E
**Decided by:** Claude (autonomous) -- the shape of what the operator decided
in §1042 (answering B-Q8): "a way to ask the terminal how wide it will draw
something as part of the protocol". The shape was left to the terminal's lane
(lane B's `b-c-the-terminal-should-answer-how-wide-it-will-draw-text`, relayed
by lane C).

**In short:** a program asks SlateOS's terminal how many cells some text takes
by writing `ESC ] 7730 ; w ; <text> ESC \` (or ending it with BEL), and reads
`ESC ] 7730 ; w ; <cells> ESC \` -- ended the way the question was -- from its
own input. The number is exactly how far printing that text would move the
cursor here. Any other terminal ignores an OSC it does not know, so a program
waits a moment for the answer and falls back to its width table.

| Call | Chosen | The other way, and why not |
|---|---|---|
| The form | a private OSC, number 7730 | a CSI (`CSI ... n`, as the cursor report is asked): a CSI cannot carry text. A DCS would do; an OSC is what xterm's own queries (colours, the clipboard) use, and a terminal that knows neither ignores both |
| The text | raw UTF-8 up to the terminator -- one grapheme cluster or a run | hex-encoded, as XTGETTCAP is: safe for any byte, but a program would encode what it is about to print. Raw is what a title carries. The cost: text holding BEL or ESC cannot be asked about, and control characters take no cell anyway |
| The answer's form | the question's OSC with the number in place of the text, ended as the question was | a CSI like the cursor report: a program that reads xterm's OSC colour answers reads this one the same way, and ending an answer as its question ended is what xterm does |
| What the number is | the cursor's advance: `put_char`'s width for each character, and none for an ASCII control | a grapheme cluster's width (an emoji sequence as 2): this terminal places each code point by the table, so a cluster answer would disagree with where the cursor goes -- and where the cursor goes is what a program lining text up needs |
| Bytes that are not UTF-8 | counted as the screen draws them: a U+FFFD for each bad byte or broken sequence | refused: a program measuring bytes it is about to print wants what printing them does |
| An OSC longer than 4 KiB | dropped whole, and not answered | cut short: a cut question would be answered about text nobody asked about |

**Also decided, in the same change:** an OSC string is read by the screen's own
UTF-8 decoder (a title was pushed a byte at a time as Latin-1); an overlong
UTF-8 form (`C0 AF` for `/`) is U+FFFD; the C0 controls the terminal does not
act on draw nothing, as in xterm (they were replacement characters); and the
ST that ends a DCS no longer dispatches the last OSC again.

**The request's second question -- does the renderer fit each glyph to the cells
the table gives it?** Yes: each glyph is drawn at its cell's x and clipped to
its one or two cells, a narrower one sitting at the left of them, and the next
character is placed by the grid, never pushed. What is not done: a combining
mark is not drawn at all -- the cell keeps only its base character
(`known-issues.md`, "The terminal draws no combining mark").
