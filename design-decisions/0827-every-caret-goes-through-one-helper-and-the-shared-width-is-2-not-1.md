## 827. Every caret goes through one helper, and the shared width is 2, not 1

**Date:** 2026-09-09
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the blinking bar that marks where your typing will go was drawn by
six different bits of code, and they disagreed about how thick it should be —
some drew it one pixel wide, some two. They now all call one function, and the
shared answer is two pixels. This also gives the "make my text cursor wider"
accessibility setting somewhere to attach, which it never had.

**What was there.** Six draw sites, three widths, no decision behind any of
them: `textedit::push_caret` drew a 1-pixel line and had three callers; the
launcher drew 2 inline; the path bar drew a 2-wide filled rectangle via its own
`CURSOR_WIDTH`; the run dialog drew 1 inline. They were written at different
times and each picked for itself.

**The choice that had to be made.** Unifying means picking, because two of the
sites have to change whichever number wins.

| | 1 pixel | 2 pixels |
|---|---|---|
| *What changes* | the launcher and path bar carets get thinner | the run dialog and the three toolkit fields get thicker |
| for | the conventional desktop caret; least visually heavy | visible on a high-resolution display, where a 1px caret nearly disappears; already what the two most-used fields had |
| against | on a HiDPI panel it is easy to lose | slightly heavier than the platform convention |

**Two pixels**, for the accessibility reason: this whole change exists so a user
who cannot easily find the caret can make it wider, and defaulting to the
thinnest possible value is the wrong end to start from. A user who prefers the
hairline can still have it — the width is a parameter, and `CARET_WIDTH * 0.5`
is one.

**A degenerate width falls back rather than drawing.** `push_caret` replaces any
width that is not finite and positive with the default. This is not defensive
padding: the accessibility config parses its multiplier with
`str::parse::<f32>`, which accepts `"nan"`, and `f32::clamp` passes NaN through
unchanged, so a hand-edited config could otherwise erase every caret in the
shell with no error anywhere — and a caret that is not drawn loses the user's
place in the text.

**What it does not do yet.** Nothing reads a user preference into it; every
caller passes the constant. The setting that should feed it is dead — see
`known-issues.md` `TD-C-THE-ACCESSIBILITY-CONFIG-IS-A-DEAD-PARALLEL-COPY`. The
value of doing this half now is that the six sites became one, so wiring the
preference later is a change in one function rather than a hunt.
