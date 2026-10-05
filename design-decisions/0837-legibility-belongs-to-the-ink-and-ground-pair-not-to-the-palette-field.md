## 837. Legibility belongs to the ink-and-ground pair, not to the palette field

**Date:** 2026-09-12
**Lane:** C
**Decided by:** Claude (operator-approved scope)

**In short:** the operator asked that text always reach the accessibility
contrast floor of 4.5:1. Some of the colours that text is drawn in are *also*
used to fill things -- the accent colour is both "the colour of a link" and
"the colour of a switch that is on". Making those colours darker so the text
can be read would also darken every switch, badge and progress bar, silently
changing something the user picked. So the palette keeps every colour exactly
as chosen, and a site that draws *text* asks for a readable version of it.

**Glossary.** *Contrast ratio* is the WCAG number for how far apart two
colours are in brightness; 4.5:1 is the level-AA floor for body text.
*Ground* is whatever a piece of text is drawn on top of -- the page, a card, a
toolbar. *Role* is a named colour in the palette (`accent`, `red`, `subtext0`).

**What the measurement forced.** The obvious fix -- lighten the cards until
everything is readable on them -- is impossible, and provably so. The palest
accent (maroon) only clears 4.5:1 against greys lighter than `#EFEFEF`, and the
page itself is `#EFF1F5`. There is no card shade *darker than the page* that
all fourteen accents survive. The ink is the only thing that can move.

**The split, which is the actual decision**

| roles | resolved | why |
|---|---|---|
| `subtext0`, `subtext1`, `link` | in the palette, at construction | they exist to be read: never a fill, never a badge, not user-chosen. 546 of the 861 sites, and not one of them changes |
| `accent`, `red`, `green`, `yellow`, … | at the draw site, via `Palette::ink` | dual-use. An accent is also a switch that is on; `red` is also an error bar |
| `overlay0` | not at all | the muted ink -- a disabled label, an empty field's placeholder. WCAG 1.4.3 exempts inactive controls, and making it legible would make a disabled control look *enabled* |
| `text`, `border` | not needed | `text` clears the floor unaided, and a border is a component outline at the 3:1 of SC 1.4.11, not body text |

**The alternatives**

**A. Adjust the palette's colours in place.** Tried first, and three existing
tests refused it -- `a_custom_accent_reaches_the_palette_exactly_as_chosen`
most directly. They were right. For: one line, no draw site changes. Against:
a user who picks `#123456` gets `#75899C` on their switches, and the swatch
they picked from no longer matches what they see.

**B. A second field per dual-use role** -- `accent` and `accent_ink`. For:
keeps both meanings, no method call at the site. Against: doubles the
categorical half of the palette, and every one of those pairs is a chance to
read the wrong one -- with no way to notice, because both render.

**C. What was chosen: a method.** `p.ink(p.accent)` at sites that draw text.
For: the palette keeps one meaning per role, custom accents survive untouched,
and it extends to a colour nobody has seen -- a ratio is a promise about every
colour in the cube, where a table is only as good as the values in it the day
it was written. Against: 315 sites to convert, and site 316 can forget.

**Where the ink goes, which the conversion had to work out.** Not "at the call
site" -- that was tried and is wrong. The rule that survived 409 sites:

> **Ink at the point where you can see that every path through it is text.**

For `osd::render_icon_text_osd` that is the function body, because its colour
parameter has exactly one use in it. For `PermissionState::color` it is the
individual arms, because one arm returns `overlay0` -- the muted ink WCAG 1.4.3
exempts -- and raising that makes a *not decided* row look decided. "Is it a
helper?" is the wrong question; "does it have an exempt path?" is the right one.
And there is a precondition that only showed up later: you must check the
*callers* too, because a colour method used for a fill as well as text cannot
be inked in its body at all.

**What the scanner cannot see, stated plainly because the number is
reassuring and wrong.** `ink-text.py` classifies by the role named at the draw
site. Four ways a colour reaches a `RenderCommand::Text` without naming one --
every one found by a failing test, none by the scanner:

| | example |
|---|---|
| a method in the `color:` field | `app.state.color(p)` |
| a helper's return value | `osd::icon_info -> (glyph, Color)` |
| an argument to a draw helper | `render_icon_text_osd(.., p.red, ..)` |
| a local passed by field shorthand | `let color = ..; Text { .., color, .. }` |

So "`--check` reports zero" means *zero among the sites it can classify*. 108
methods return a themed colour and name no role at a draw site; they are
`TD-C-FORTY-NINE-COLOUR-METHODS-ARE-INVISIBLE-TO-THE-INK-SWEEP`. The scanner
grew a `--blind` mode to report that gap rather than imply its absence.

**About that last cost.** It is the same objection §829 answered for surfaces,
and it gets the same answer: a scanner. `gui/appearance/ink-text.py` performs
the conversion and, run without `--apply`, counts what is left -- so "did
anyone forget?" is a question with a number rather than a matter of care. The
day's other lesson applies here too: the check has to be *reachable*. The
membership sweep that 45 modules rely on was about to start failing, because
an inked accent is a legitimate colour of the palette that is not one of its
roles; teaching `is_accounted_for` about it, in the one shared place, was part
of the same change rather than 45 modules each declaring the same eleven
values.

**What is deliberately not decided here.** Whether the *card* theme's shade
ladder should be lightened as well. It cannot fix this on its own -- see the
measurement above -- but it would reduce how far the inks have to move. That
is a question about how the optional theme looks, and it is the operator's.
