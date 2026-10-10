## 1460. The taskbar's glass and spacing are a theme axis; how see-through it is stays the user's

**Date:** 2026-10-01 &middot; **Decided by:** Claude (autonomous) &middot;
**Lane:** C, with E

**In short:** The taskbar always wore the Aero look's glass -- shiny lines
along its top, a glow, a shade, glossy buttons -- whatever theme was
chosen. A theme can now say how much of that glass the taskbar wears, from
all of it to none (a flat bar in the theme's colour), and how far apart the
taskbar's buttons are. The user still decides, separately, whether the bar
is see-through. The built-in theme looks exactly as before. Choosing a theme
for this in Settings is lane E's (`requests/c-e-choose-the-taskbar-panel-in-settings.md`).

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| What the axis sets | `gloss` (0 to 1) and `spacing` (between tiles, after the start button, between the sections) | every one of the reference's two dozen strengths and margins as its own setting | one knob gives what the roadmap asks -- a flat bar "without losing the rest of the default visual identity" -- and keeps the glass consistent with itself; a theme that wants other strengths draws on the colours axis, which sets what the glass is laid over |
| What gloss scales | the bar's lines of light, glow and shade; a tile's sheen, top highlight and faint body; the start orb's shade, gloss and inner line | everything translucent | edges, hover marks, shadows and the orb's rings are how a flat bar's parts are still told apart; they are not the glass |
| Transparency | stays the user's `taskbar_style` and `transparency` | a theme's opacity | whether one can see through the bar is an accessibility and taste choice the user already makes; a theme overriding it would undo it every time the theme changed |
| Blur | not a setting yet | a theme's blur strength | the compositor's blur behind the bar has no strength to set (lane F's); a setting nothing reads would be the §856 failure |
| The section gap narrower than a tile gap | drawn as the tile gap | as written | the divider's gap is there to widen the space; it must never be the narrowest on the bar |
