## 844. The tray's icon run is bounded by a share of the taskbar, not a count

**Date:** 2026-09-13
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Programs can put little icons in the corner of the taskbar
next to the clock. Nothing stopped a program from adding hundreds, and
when it did, the icons covered the clock and squeezed the buttons you use
to switch between windows down to nothing — so the taskbar stopped working
and it did not look broken, it just looked full. The icons now get at most
a quarter of the bar's width, and anything that does not fit goes behind a
small arrow you can click to see the rest. The choice recorded here is
*how much* they get: a fraction of the bar rather than a fixed number of
icons.

### What was actually wrong

Measured on a 1920-pixel-wide taskbar with eighty icons registered:

| | before |
|---|---|
| `tray_width` | 2167, on a bar 1920 wide |
| `tray_x` | 0 — the tray claimed the entire bar |
| the clock | at x=1805, underneath icons 75 to 80 |
| `taskbar_button_rect(0).w` | **0** |

No clamp was missing as such; the clamps were all there and all did the
wrong thing, because each was written for the case of a tray slightly too
wide rather than a tray twice too wide. `.max(0.0)` on the window buttons'
remaining width turns "negative room" into "no buttons" without
complaining.

`MAX_TRAY_ICONS` is 4096 and a program chooses its own icon ids, so this
was reachable by any process that could open a connection. It is not a
privilege escalation — nothing is read or written that should not be — but
it is a denial of the shell by an unprivileged client, which is the sort
of thing a microkernel design is supposed to make hard rather than easy.

### The decision

**A share of the taskbar's width (a quarter), not a fixed icon count.**

| | share of the bar | fixed count |
|---|---|---|
| 1024-wide display | 10 icons | 12 icons, crowding a bar that has no room |
| 3840-wide display | 40 icons | 12 icons, wasting two thirds of the space |
| a larger UI scale | fewer icons, correctly | the same 12, each bigger |
| states the invariant? | yes: the taskbar keeps three quarters | no: it happens to work at 1920 |

The case for a fixed count is that it is predictable and is what Windows
does — a user who has arranged eight icons sees eight icons on every
machine they log into. That is a real benefit and it is why the number is
worth revisiting if per-user tray settings ever exist.

It loses here because the question being answered is not "how many icons
should a person see" but "how much of the taskbar may a program take", and
that is a width. A count only answers it by accident, at one resolution and
one scale factor; change either and the same twelve icons are a different
fraction of the bar. The defect above was a width overflowing, and a bound
expressed in the wrong unit would have to be re-tuned every time a display
changed — which is to say it would be wrong on someone's machine and right
on the developer's.

### What follows from it

- **The limit is not stored anywhere.** It depends on the bar's width and
  the display scale, neither of which the tray is told about when they
  change. A cached count would be a flag someone has to refresh on resize,
  and forgetting is silent: icons behind a chevron that has room for them,
  or drawn off the edge. `TrayIconArrangement` therefore does *not* hold a
  `max_visible`; the limit is passed in at each use.
- **The cap is never applied without the chevron.** A bound that just stops
  drawing is the defect the whole tray effort exists to remove: a program
  with an icon nobody can click. One slot of the budget is the chevron's
  whenever anything overflows.
- **A quarter is a number, and it is the part of this worth arguing with.**
  It is not derived from anything; it is a quarter because three quarters
  is enough taskbar. If tray icons ever become something a user arranges
  deliberately rather than something programs do to them, this should
  become a setting.
