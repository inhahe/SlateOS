## 1406. The start menu lights what the pointer is over, apart from the keyboard's row

**Date:** 2026-09-26 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** Moving the pointer over the start menu used to change nothing on
screen until a click, so a user could not see which row a click would reach --
the reference lights each thing under the pointer (its `:hover` rules). Now
the start menu does too: a program's row, a place, "Shut down", the caret and
a power choice each light while the pointer is over them. The row the
keyboard has chosen keeps its own, stronger mark, and the two can be on
different rows at once.

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| Does the pointer move the keyboard's row? | No: the light is separate, and quieter | the pointer *selects* (Windows 7's start menu) | the reference's lights are pure `:hover`; and a search's first result stays what Enter starts, however the pointer drifts while typing |
| What a row's light is keyed to | its place on screen (`StartLit::Row`) | the program in it | the list scrolls under a resting pointer; the light must stay on the row a click there would reach |
| Headings | never lit | lit like a row | a heading is not something to click |
| When the menu closes | the light is forgotten | kept for next time | a menu opened again lights nothing until the pointer is over it, rather than what the pointer left |
| Colours | the accent for the list and the power choices, the column's text colour for the places and the power button | the reference's fixed blues and whites | the reference's are its blue theme's; taken as the accent and the text colour they follow the user's theme, as every other colour here does |

The caret, a chevron alone, also names itself -- "Power options", the
reference's `title` -- with the tooltip the taskbar's pins and tray icons use.
