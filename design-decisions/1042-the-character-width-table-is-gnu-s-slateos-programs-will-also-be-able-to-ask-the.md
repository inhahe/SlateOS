## 1042. The character-width table is GNU's; SlateOS programs will also be able to ask the terminal

**Date:** 2026-09-27
**Lane:** B
**Decided by:** Claude (operator-approved scope). Answering B-Q8, the
operator left the choice of table to Claude ("just do whichever looks the
best, or whichever you want") and asked for a width query in the terminal
protocol; Claude took its own recommendation, (a). Answer relayed verbatim
through lane F's session.

**In short:** a terminal draws text in fixed cells, and every program that
lines text up needs to know how many cells each character takes. Our one
shared table matched bash's; the GNU tools ship a newer table that differs
on 626 characters, almost all invisible marks and unassigned code points.
We now use GNU's. Separately, the operator wants SlateOS's terminal to be
able to *answer* "how wide will you draw this?", so programs written for
SlateOS can ask instead of trusting a table; that is lane C's terminal, and
has been requested from them.

**The operator's answer, verbatim:**

> I want a way to ask the terminal how wide it will draw something as part
> of the protocol. It will be an addition purely for programs made for Slate
> OS, so it won't interfere with POSIX or whatever, is that fine? Regarding
> which table to keep, you say that bash and GNU tools will not be running
> on Slate OS, only reimplementations, so does that not mean that,
> regardlress of which we keep, nothing will show up improperly (because our
> own implementations will naturally know the correct table to use)? If
> that's the case, just do whichever looks the best, or whichever you want.
> Though one thing that concerns me is the decision to make the renderer
> obey the table rather than vice versa--what if the glyph it's trying to
> render doesn't agree with the table in the current font? Wouldn't you
> either get a glyph printed too wide or too narrow?

**Why GNU's table (a).** It is a pinned upstream -- gnulib's, Unicode
15.1.0 -- that can be re-derived mechanically, where ours came from whichever
Python the build machine had. Six utilities consult a width against one
shell, so matching GNU byte-for-byte in `ls`, `wc -L` and `column` is worth
more than matching bash's line editor on characters nobody types. The
operator's premise is right: nothing shows up improperly either way, because
every program on SlateOS reads the one table.

**"Is a private query fine?" -- yes, with one limit.** An escape sequence
only SlateOS programs send, which other terminals simply do not answer, is
exactly how terminals have always been extended (xterm's own queries began
that way). The limit is that a query needs a terminal on the other end: `ls`
choosing columns for a pipe or a file, or a program running over ssh from a
machine with an older table, still has only its table. So the query
supplements the table rather than replacing it.

**The renderer and the font (the operator's concern).** In a terminal the
cell grid is authoritative, not the font: every program on the screen, and
the cursor, computes positions from the table, so the renderer has to fit
each glyph into the cells the table gives it. A glyph narrower than its cells
is placed inside them; a wider one is scaled or clipped to them, and never
pushes the following characters out of place. The concern is real in one
form: a font whose glyph for a one-cell character is drawn wide looks
cramped. The remedy for that belongs to the font side -- a fallback font, or
scaling -- and not to a table that varies by font, because then the layout
of text would depend on which font is installed, which a program writing to
a pipe or over ssh cannot know. Lane C has been asked to confirm the renderer
fits glyphs to cells this way.

**What follows:** `userspace/charwidth` takes gnulib's table; the `ls`
harness's two permanently-deferred cases become agreeing ones; `osh`'s line
editor stops matching bash on those 626 (recorded where it is measured).
Request to lane C: the width query, and the fitting rule above.

**Where:** `userspace/charwidth/src/lib.rs`; `known-issues.md` ->
`TD-B-OUR-WIDTH-TABLE-IS-BASHS-AND-COREUTILS-9.5S-IS-NOT`.

**Corrected the same day (2026-09-27), on measurement, before the table was
touched; applied 2026-10-01.** The correction is Claude's, inside the same
operator-approved scope ("whichever looks the best"), so it is as revisable as
the choice it corrects. Dumping gnulib's `uc_width` for every code point from coreutils
9.5's own `libcoreutils.a` and comparing it with our table gives the known 626
differences in 71 runs (plus NUL), and they are not one kind of thing:

| Kind | Where | Ours | gnulib's | Better-looking |
|---|---|---|---|---|
| **Unicode version.** Ours came from Unicode **16.0** (the build machine's Python), gnulib's is **15.1** | emoji and symbols new or made wide in 16.0 (U+1FA89, U+1FAE8, U+1D300-1D356, U+4DC0-4DFF), and the scripts 16.0 added | 16.0's | 15.1's: 1 for anything 15.1 had not assigned | **ours** -- a new emoji given one cell overlaps its neighbour |
| **Soft hyphen** U+00AD | common in pasted web text | 1 | 0 | **gnulib's** -- Unicode shows it only at a line break; width 1 draws a stray hyphen mid-word |
| **Prepended concatenation marks** U+0600-0605, 06DD, 070F, 0890-0891, 08E2, 110BD, 110CD | Arabic number signs and kin | 0 | 1 | **gnulib's** -- a visible sign given no cell piles onto the next character |
| **Hangul Jamo Extended-B** conjoining medials and finals U+D7B0-D7FB | Old Korean | 1 | 0 | **gnulib's** -- they combine with the syllable, as U+1160-11FF (which both tables zero) do |
| **Unassigned code points in East Asian blocks** (U+3040, U+3097-3098, U+FF00, U+1F203-1F20F, ...) | nothing yet | 1 | 2 | neither, visibly -- no text contains an unassigned code point. UAX #11's own defaults (EastAsianWidth.txt's `@missing` lines) make only the ideograph blocks wide before assignment; gnulib rounds up whole blocks by its own list. The standard's rule is taken |
| **Kannada** U+0CBF, U+0CC6 | real Kannada text | 0 (non-spacing marks, `Mn`; glibc agrees) | 1 | ours |
| **NUL** | -- | none: a control | 0 | ours -- NUL ends a C string; it is not a zero-width character |

So neither table as it stands is the best-looking one, and "(a), gnulib's" is
revised to what the operator actually asked for: **the newest Unicode data
(18.0.0), pinned by SHA-256 rather than taken from whichever Python builds it,
with gnulib's three rendering policies** -- soft hyphen 0, prepended
concatenation marks 1, conjoining Jamo 0 -- **and UAX #11's defaults for
unassigned code points**. `scripts/charwidth-gen.py` generates it. No upstream
matches that byte for byte, so each harness that measures widths (`ls`,
`wc -L`, `osh`'s `select`, `column` and the `libsmartcols` programs) records
the code points where it differs on purpose.

The rationale above also said GNU's table buys agreement in "`ls`, `wc -L` and
`column`". `column` was wrong: util-linux measures with **glibc's** `wcwidth`,
and since B-Q8 was written lane B has ported `column` and ten programs built on
`libsmartcols`, all reading this table. Taking gnulib's wholesale would have
traded bash *and* util-linux for coreutils, not bash alone.

**One more table, found on the way.** SlateOS's C library (`posix/src/wchar.rs`,
lane D's) carries a third, hand-written `wcwidth`, so a C program ported to
SlateOS measures with neither. One table for the system means libc's `wcwidth`
answering from the same data; that is requested from lane D
(`requests/b-d-libc-wcwidth-should-answer-from-the-one-width-table.md`).

**And a notice.** A table generated from the UCD's files is a modified copy of
them, and the Unicode licence asks for its notice with every copy, so
`userspace/charwidth/licenses/notices.yaml` names the Unicode Character
Database at the version the tables come from (§1433's gatherer carries it into
the image); `charwidth-gen.py --emit` keeps that version in step and `--check`
refuses a mismatch.
