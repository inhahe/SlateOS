## 1446. How the desktop moves is one value: a theme's animation at the user's speed, carried on the palette

**Date:** 2026-09-29 &middot; **Decided by:** Claude (operator-approved scope:
`roadmap-detailed.md` → *Tier 3 — Animation Tuning* asks for the three
settings; their spellings, bounds and curves, and how they meet the user's
speed, are Claude's call) &middot; **Lane:** C

**In short:** A theme can now say how the desktop moves -- how long a panel
takes to open, whether things glide in and settle, go at a constant speed,
or spring a little past their place -- or that nothing moves at all. The
user picks which theme's motion to use separately from its colours, as with
icons and control shapes, and their own animation speed (Off, Fast, Normal,
Slow) then speeds it up or slows it down. Before this, the speed setting
reached only a part of the shell that draws nothing: the overview still
faded in and the notification pane still slid at their own fixed lengths,
even with animation set to Off. Now everything that moves on the desktop
follows the one setting.

**The file.** An `animation` section in `theme.yaml`, named as the axis is
in `meta.supports`; `theme.animation: <name>` in `appearance.yaml` chooses it,
next to `theme.colors`, `theme.icons` and `theme.widget_style`. The shipped
`aero/theme.yaml` writes every setting out as the template:

| Setting | Values | The built-in theme's |
|---|---|---|
| `enabled` | `true`, or `false`: nothing moves | true |
| `duration-ms` | the standard transition, 50-1000 ms | 200 |
| `easing` | `ease-out`, `linear` or `spring` | ease-out |

As with every section, what it leaves out is the built-in theme's, and a
value that cannot be read costs that value and is listed for the theme's
author. A duration outside 50-1000 is taken as the nearer end, with a note;
a zero is told to write `enabled: false`, which is what it meant.

**The model: every transition is stated against a standard.** Each moving
thing keeps the length it was designed at -- the overview's 200 ms fade,
the taskbar's 200 ms slide, the on-screen display's 150 ms in and 300 ms
out, a toast's 250 ms -- and `Motion::duration_ms` scales each by the theme's
standard over the built-in 200 ms, and by the user's speed. So one setting
moves the whole desktop together.

| Option | For | Against |
|---|---|---|
| **One standard that scales every transition** (chosen) | the roadmap's `animation-duration-ms` is "the global default"; the transitions keep their proportions (a fade out stays twice its fade in); one number a theme author can reason about | a theme cannot lengthen one transition alone |
| One duration for everything | simplest | the overview's fade and the display's 300 ms fade out would become the same length, which they were designed not to be |
| A duration per transition in the theme | finest control | a theme would have to know the desktop's inventory of animations, and every new one would be a new key |

**The curves, and why arriving and leaving differ.** Under `ease-out`
something arriving decelerates into place (cubic) and something leaving
accelerates away (quadratic -- a cubic start is so slow that a closing panel
seems to hesitate). `spring` arrives past its place by 8.7% and settles --
`1 - e^(-7t) cos(2.5 pi t)`, exactly 1 at the end so nothing jumps -- and
leaves as ease-out does, since overshooting "gone" shows nothing. `linear` is
a constant speed both ways. A panel anchored to the screen's edge (the
notification pane, the auto-hidden taskbar) and an opacity clamp the
overshoot; a toast floating on the desktop keeps it.

**A transition turned round starts where it is drawn.** With different
curves out and in, where a slide's clock stands is not where the thing is
drawn -- half-way through leaving is a quarter gone, half-way through
arriving is seven-eighths there -- so the notification pane and the taskbar,
reversed mid-slide, start the new slide at the moment its own curve has them
where they are (`Motion::when_arriving_at` / `when_leaving_at`). The taskbar
used to jump to fully hidden when the pointer came back mid-slide; it no
longer does, under any curve.

**How it meets the user's speed.** `Motion::at_speed` multiplies the theme's
standard by the speed's multiplier -- Fast 0.75, Slow 1.5 -- and Off is the
still motion, as is a theme's `enabled: false`. Applied once, where the
palette is resolved (`AppearanceSettings`' `PaletteSource::motion`), so
nothing downstream has a second definition of "slow".
`AppearanceSettings::animations_enabled` stays the user's switch alone: a
theme without transitions is a look, not the user asking for less motion,
and the picture viewer, which asks it before playing an animated picture,
should not stop because of a theme's taste in panel slides.

**How it reaches what moves.** On the palette (`Palette::motion`), as the
widget style is (§1435): a palette already reaches every application through
`oswindow`, so an animator needs no new argument, and the notification
daemon takes it from `theme_changed` like any application. The shell pushes
it from its one door for appearance (`DesktopShell::set_appearance`) to the
notification pane and the on-screen display, and the session to the
animation manager and auto-hide (`ShellSession::sync_motion`); the overview's
fade takes it when it begins. High contrast keeps the motion whole: it is
about telling things apart.

**What a motion does not choose:** whether a busy indicator turns (that is
state, and someone who turned animation off has not asked to stop being told
the machine is working); where anything ends up; and how long anything stays
-- a toast's time on screen, a tooltip's delay, the taskbar's hide delay are
waits, and settings of their own.

**Found on the way:** a scale of zero (Off) left the animation manager's
animations frozen part-way and asking for a frame every frame; setting it now
ends them where they were going. And two doc comments had drifted onto the
wrong functions in `session.rs` and one in `taskbar_autohide.rs`.

**Not yet following it:** the compositor animates no windows yet; when it
does, it reads the motion from its palette (lane F). `window_peek.rs` is
dormant -- nothing in the shell drives it -- and takes the motion when it is
wired. The notification daemon follows the motion but not yet the colours
(`TD-C-THE-TOAST-DAEMON-DRAWS-IN-ITS-OWN-COLOURS`). Lane E's Settings page
offers the choice (`requests/c-e-a-theme-can-set-the-motion.md`).

**Revisit if** a theme author needs one transition longer than the others,
or a curve the three do not cover.
