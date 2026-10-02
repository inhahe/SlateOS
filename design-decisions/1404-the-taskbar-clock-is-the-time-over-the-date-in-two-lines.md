## 1404. The taskbar clock is the time over the date, in two lines

**Date:** 2026-09-26 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The clock in the taskbar's corner was one line --
"Tue Aug 18 16:30". It is now two, as the Aero reference draws it: the time,
bold, and under it the weekday and date, smaller and a little dimmer, each
centred. It takes the width of its wider line rather than of both side by
side, which is room the window tiles get back. With the weekday and date both
switched off, or in a bar too short for two lines, it is one line as before.

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| Layout | two lines, the time bold over the date at the caption size and 0.85 of the text colour | one line | the reference's `aero-clock`; the slot narrows by the date's width |
| A bar too short for two lines | one line, the date before the time, as before | two lines squeezed | a clock cut through the middle of its digits is worse than one set out in a line |
| What the date line says | the switches' weekday and date, "Tue Aug 18" | the reference's "2026/05/06" | the Date & Time panel's switches already say which parts to show; the year is the field nobody reads at a glance (§492) |
