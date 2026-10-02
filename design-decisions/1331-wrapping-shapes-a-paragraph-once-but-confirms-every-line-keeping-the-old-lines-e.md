## 1331. Wrapping shapes a paragraph once but confirms every line, keeping the old lines exactly

**Date:** 2026-09-27
**Lane:** F
**Decided by:** Claude (autonomous) -- answering lane E's request
`e-f-wrap-reshapes-the-whole-line-for-every-word.md`, which asked for the
same lines, faster.

**In short:** breaking a paragraph into lines used to measure the whole line
again for every word added. Now the paragraph is measured once to guess each
line's end, and each guess is checked by measuring that line on its own, so
the lines come out exactly as before at about a sixth of the cost.

**Alternatives.**

| | For | Against |
|---|---|---|
| Propose from one shaping, confirm each line (chosen) | the old lines exactly, whatever the font's shaping does at a space | two shapings per line: about 6x, not 20x |
| Break from the paragraph run's advances alone | one shaping in all | a line can come out a kern wider or narrower than it is drawn, so a break can move |

**How to reverse.** `Paragraph::extend` in `gui/font/src/shape.rs`: dropping
the two `fits` checks trusts the run.
