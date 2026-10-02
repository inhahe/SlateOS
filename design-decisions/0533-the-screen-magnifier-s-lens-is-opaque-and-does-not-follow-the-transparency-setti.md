## 533. The screen magnifier's lens is opaque, and does not follow the transparency setting

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The screen magnifier draws a lens that follows the pointer and
shows an enlarged copy of whatever is under it. That lens used to be
see-through, so the *normal-size* desktop showed through the enlarged copy of
itself — two overlapping pictures of the same words, at two different sizes,
exactly where someone who cannot read small text is looking. It is now solid.
The choice worth recording is that it stays solid even for a user who has
asked the rest of the shell to be see-through.

**Glossary:** *alpha* is how opaque a colour is (255 = solid, 0 = invisible).
The shell has a *transparency setting* with four positions, which the palette
exposes as `panel_alpha` (Off = 255, Subtle = 230, Moderate = 200, Full = 160)
and which every floating panel — taskbar, start menu, notification popup —
reads.

**What was there.** The lens background was `Color::rgba(30, 30, 46, 200)` for
the circular and rectangular shapes and `Color::rgba(30, 30, 46, 220)` for the
docked strip. `30, 30, 46` is Mocha `base`, so the colour was a leftover copy
of the palette and had to change regardless. The *alphas* are the interesting
part: two different values for two shapes of the same lens, with no comment and
no test mentioning either. Nothing chose them.

**The options.**

1. **Keep them.** *What changes:* nothing — the enlarged content keeps sitting
   on top of a smaller copy of itself, and the two lens shapes keep disagreeing
   about how much.
2. **Follow `panel_alpha`.** *What changes:* the lens is solid only for users
   who set transparency to Off; at Full it becomes *more* see-through than it
   is today.
3. **Opaque always.** *What changes:* the lens is solid whatever the
   transparency setting says — the only overlay in the shell that does not read
   it.

**Decided: option 3.** Transparency everywhere else in the shell is decoration
laid over content the user is not reading — a taskbar over a wallpaper, a menu
over a window they have looked away from. Behind this lens is not other
content; it is *a second rendering of the very thing being read*. Any alpha at
all therefore produces a double image at the point of regard, and it does so in
the one feature whose users are least able to disentangle it. A preference that
can switch off an accessibility aid is not a preference the aid should honour.

Option 2 is the one that looks principled and is not: it makes the lens
correct for exactly one of the four settings and silently degrades the feature
for the other three, which is worse than option 1 in the sense that matters —
it would be *defensible*, so nobody would revisit it.

**The cost, stated rather than left to be discovered.** A user who likes
translucent panels will find this one solid, and there is no setting that
changes it. That is a real inconsistency and it is deliberate; the module
header says so at the site, so the next person to notice finds the reasoning
instead of a bug. If a reason ever emerges to make the lens translucent — a
compositor that can magnify *without* redrawing the region underneath, which
would remove the double image — this decision is the thing to revisit.

**Regression pin.** `the_lens_is_opaque_in_both_modes` renders both lens shapes
in both modes and requires alpha 255 on every fill. The related ink change is
pinned separately by `the_crosshairs_are_legible_on_the_lens_in_both_modes`:
the crosshairs used to be white at alpha 128, which over Latte `base` measures
**1.06:1** — a hairline no one can see, in a magnifier. They are now
`readable_on(p.base)` at full alpha, 14.50:1 dark and 16.58:1 light.
