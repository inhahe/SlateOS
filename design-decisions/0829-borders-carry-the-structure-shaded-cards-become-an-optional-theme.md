## 829. Borders carry the structure; shaded cards become an optional theme

**Date:** 2026-09-11
**Lane:** C
**Decided by:** Operator (Claude built the comparison page and laid out the options; the operator chose borders and specified the colours)

**In short:** boxes on screen were told apart by filling them with slightly
different greys. They will now be told apart by drawing a line around them
instead, on a plain background. The grey-filled look stays available as a theme
someone can switch on. The operator also fixed the colours: black lines and
black headings, a muted blue-green for secondary text, for the line around the
selected thing, and for a switch that is on.

**The specification, as given**

| role | value |
|---|---|
| border, unselected | black |
| background | white or off-white |
| border, selected | a blue-green, "not too bright" |
| secondary text (`subtext1`) | the same blue-green |
| toggle switch, on | the same blue-green |

**Two things follow from that spec rather than being chosen freely, and both
are recorded because they change the palette beyond what was asked.**

1. **The blue-green replaces the blue accent; it does not join it.**
   Selected-border, toggle-on and secondary-text *are* the accent's roles. Every
   blue-green dark enough to clear 4.5 : 1 lands **1.19–1.74** from the existing
   `#0036A3` — at or below the 1.30 that reads as "barely distinct". Keeping
   both would have put two indistinguishable blues in one palette, which is the
   §826 flattening again in a different pair.
2. **`subtext0` takes the same value as `subtext1`.** They were **1.10** apart
   in the shipped palette — "one colour" by the measure the explorer uses — so
   there was no second grey to preserve. Worth flagging rather than burying:
   `subtext0` has **1,087** uses to `subtext1`'s **161**, so if these two should
   in fact differ, `subtext0` is the one that carries the weight.

**The colour is `#00688B`** — revised the same day, on "make it more bluish,
either cerulean or cyan". Hue 195 and fully saturated, so it reads as cerulean
rather than the teal first proposed (hue 173): 6.26 : 1 on white, 5.54 on the
off-white page, 3.35 from black.

Neither obvious spelling of the ask survives the floor, which is why the value
is not simply "cerulean" or "cyan":

| candidate | on the off-white page | |
|---|---|---|
| classic cerulean `#007BA7` | **4.23** | passes on pure white, fails on the page we use |
| pure cyan `#00FFFF` | **1.25** | not a text colour at any size |
| **`#00688B`** | **5.54** | the most saturated cerulean that clears the floor with margin |

The operator can retune it live; the explorer carries it as the
`Borders (the default)` preset.

**Why this is the right shape, beyond being what was asked.** The operator's
mother supplied the argument without meaning to. She wanted the selected menu
item and the selected settings row to match, and under shaded cards they cannot:
selection there is a *step*, not a colour — a thing lifts one rung above
whatever it sits on, and those two sit on different things. Making them match
means `surface0 = surface1`, which erases the distinction between a selected
settings row and an unselected one. **A border does not have that problem**: an
outline means "selected" and means it identically everywhere, because it does
not depend on what is underneath. That is a property shades cannot be given.

**What this costs, honestly.** Up to 1,713 draw sites currently pick a fill.
The conversion is mechanical but wide, and the shaded theme has to keep working
throughout, which means the ladder cannot simply be deleted.

**What is still open.** The card theme's contrast. The operator's own words:
"I guess we still have to figure out how to color them so that contrast is
always >= 4.50." That is unchanged by this decision and is now scoped to an
optional theme rather than the default, which lowers its urgency without
removing it. `TD-C-THIRTEEN-LIGHT-ACCENTS-STILL-FAIL-ON-CARDS` is part of the
same problem.

**Supersedes** the open half of §826. The inks chosen there were chosen against
a shaded background; `main` survives unchanged at `#000000`, `page` survives at
`#EFF1F5`, and the accent and secondary roles are replaced by the above.
