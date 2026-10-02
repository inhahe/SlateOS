## 826. The light theme's text inks, chosen by the operator: `#000000`, `#373739`, `#0036A3` on `#EFF1F5`

**Date:** 2026-09-09
**Lane:** C
**Decided by:** Operator (Claude asked C-Q10 with four options and recommended A, then withdrew that recommendation on measuring it; the operator supplied values instead of choosing, and they are better than any of the four)

**In short:** in the light theme, the smaller grey text and the accent-coloured
text were too faint to read wherever they sat on a shaded box rather than
directly on the page — about 850 places. The operator picked new colours: black
for main text, a very dark grey for secondary text, a deep blue for links, on
the existing off-white page. They are now in use and a test enforces them.

**The values**

| role | was | now |
|---|---|---|
| main text (`LIGHT_TEXT`) | `#4C4F69` | `#000000` |
| secondary text (`LIGHT_SUBTEXT1`) | `#5C5F77` | `#373739` |
| default accent (`LIGHT_BLUE`) | `#1D62EC` | `#0036A3` |
| page (`LIGHT_BASE`) | `#EFF1F5` | `#EFF1F5` — unchanged |

**Why this beat every option the question offered.** C-Q10 put four to the
operator (darken the greys, lighten the cards, forbid text on dark cards, do
nothing) and recommended the first — then withdrew that recommendation, because
measuring it showed the greys would have to go so dark to clear the greyest card
that all the inks landed at *identical* luminance: body text, captions and links
weighing exactly the same, with hue the only thing left to tell them apart, and
hue is the channel colour-blind vision cannot use. It traded one accessibility
defect for another.

The operator's values do not, because they move the *card* as well as the ink.
Measured across every surface in the theme:

| ink | page | surface0 | surface1 | surface2 |
|---|---|---|---|---|
| main | 18.57 | 13.60 | 11.55 | 9.71 |
| secondary | 10.50 | 7.69 | 6.53 | 5.49 |
| accent | 9.03 | 6.61 | 5.62 | 4.72 |

Everything clears the 4.5 floor, and main-to-secondary separation is 1.77 where
option A would have given 1.00.

**Three things the decision did not settle, recorded so they are not mistaken
for having been.**

1. **The card colour.** The operator also gave `#A0AECA` for "Card (shaded)".
   It is not applied: the theme has five shaded surfaces in use and the palette
   names one, so how it maps is still open — and the operator is now weighing
   whether cards should be shaded at all or delineated by borders.
   `scripts/contrast-explorer.html` renders the options.
2. **`LIGHT_SUBTEXT0`.** A third grey role that the palette does not name, so a
   value was derived: `#3D3D3F`, the lightest that still clears 4.5 on
   `surface2`. It sits 1.10 from `LIGHT_SUBTEXT1` — the same colour to any eye.
   That is forced by the surface, not chosen: `surface2` gives 9.71:1 against
   pure black, so every ink clearing 4.5 on it is crowded into the top of that
   range. A three-level grey hierarchy cannot exist on a card that dark.
3. **The other thirteen accents.** All still fail on every card (2.33–3.52).
   They were derived together by scaling each Latte accent until it cleared 4.5
   *on the page* and nowhere else — the same mistake as the greys, made once and
   applied fourteen times. `TD-C-THIRTEEN-LIGHT-ACCENTS-STILL-FAIL-ON-CARDS`.

**What enforces it.** `light_inks_clear_the_contrast_floor_on_every_surface`
checks every ink against every surface and names the failing pair;
`the_light_inks_are_not_all_the_same_weight` asserts the hierarchy option A
would have destroyed. The first guards the operator's explicit instruction to
leave the accent's thin margin (4.57, clearing the floor by 0.07) and add a
guard rather than adjust the colour.

The guard deliberately excludes the thirteen accents, because asserting a known
failure means either a red build or a muted test — it grows to cover them when
item 3 is decided.
