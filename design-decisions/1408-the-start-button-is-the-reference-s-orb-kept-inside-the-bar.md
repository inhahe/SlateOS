## 1408. The start button is the reference's orb, kept inside the bar

**Date:** 2026-09-26 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The start button was a square with a small picture on it. It is
the Aero reference's orb now: a round, glossy button in the accent with the
start picture on it, a ring of light round it and a shadow under it, glowing
while the pointer is on it -- with three differences from the reference, each
forced or chosen for a reason below.

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| Size | 36 across on the default 40-high bar: the reference's 42, but kept 2 clear of the bar's edges | 42, rising 5 above the bar as the reference's does | the taskbar's surface ends at the bar's edge, so anything above it is cut off; making the surface taller would put a band of it over the windows' bottom edge, where it would take their clicks |
| What is on it | the icon theme's `start-here`, in whichever of black or white reads on the accent | the reference's logo | there is no logo in the repository (`open-questions.md` C-Q28); it goes in the orb's place when there is |
| While the menu is open | the orb glows, as under the pointer | no mark, as the reference | the bar has always shown that its menu is open (the button was drawn pressed), and a menu opened from the keyboard has no pointer over the orb to say where it came from |
| The gloss | a bright cap over a softer skirt, and a shade at the foot: translucent white and black pills inside the circle | the reference's radial gradients | the renderer draws no gradients; the tiles' glass (§1400) is the same two-step light |
| The button's width | 64, the reference's | 48, as it was | the orb needs the room, and the rest of the bar follows it |

A press anywhere on the button still opens the menu: the orb is drawn in it,
and the target stays the button, which is larger.
