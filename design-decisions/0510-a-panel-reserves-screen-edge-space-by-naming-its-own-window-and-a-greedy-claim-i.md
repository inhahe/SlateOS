## 510. A panel reserves screen edge space by naming its own window, and a greedy claim is clamped rather than refused

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous) — lane C

**In short:** A taskbar needs a way to tell the compositor "keep the bottom 40
pixels of this screen for me, and stop tiled windows from filling into it."
Without that, every window you maximize or snap slides its bottom edge under the
bar. The question was how the taskbar should say *which screen* it means, and
what should happen when a program asks for more of the screen than it ought to
have. The answers chosen: it names the screen by naming its own window (whatever
screen that window is on is the screen it means), and an excessive request is
quietly cut down to a third of the screen instead of being refused.

### The problem

`TD-C-COMPOSITOR-TILES-UNDER-THE-TASKBAR`. Every tiling route in the compositor
divided the whole monitor, including the strip a panel occupies. There was no
protocol for a client to reserve that strip — the standing art is X11's
`_NET_WM_STRUT_PARTIAL` and Wayland's `zwlr_layer_shell` exclusive zone, and
`gui/remote` had neither.

### Decision 1 — the wlr exclusive zone shape, not `_NET_WM_STRUT_PARTIAL`

| | `_NET_WM_STRUT_PARTIAL` (X11) | exclusive zone (wlr) — **chosen** |
|---|---|---|
| Shape | twelve numbers: four thicknesses plus a start and an end offset for each | one number: a thickness, on a surface already attached to an output |
| Naming the output | offsets are measured along the *virtual desktop*, so the output is implied by arithmetic | the surface is on an output; nothing to imply |
| Two panels on one edge | side-by-side spans, each declaring the segment it occupies | thicknesses add |
| *What changes:* | a panel could claim "the left 300 px of the bottom edge" and leave the rest usable | a panel claims a full-width band; two panels on one edge stack |

`_NET_WM_STRUT_PARTIAL`'s extra ten numbers exist for one case: two panels
sharing an edge side by side. That case is rare, the encoding for it is the part
of the protocol implementations get wrong (the offsets are in virtual-desktop
coordinates, which nearly every panel and every window manager has at some point
disagreed about), and getting it wrong produces a work area that is subtly wrong
rather than obviously broken. A full-width band is what a taskbar actually is.

**Cost accepted:** a half-width dock on the same edge as a taskbar reserves a
full-width band, wasting the pixels beside it. If that ever matters the answer
is a second request that takes a span, not retrofitting spans onto this one.

### Decision 2 — the reservation names the panel's own window

The protocol has no output ids, so "which monitor" had to be expressed some
other way.

| Option | *What changes:* |
|---|---|
| Add output ids to the protocol | a panel asks for output 1's bottom edge; a stale id after a hotplug is a new failure mode, and the compositor needs a side table keyed by output to know when to drop the claim |
| Name a rectangle | a panel describes the strip; the compositor has to work out which monitor that is anyway, and two panels can describe overlapping strips |
| **Name the panel's own window** (chosen) | a panel asks for the bottom edge "of the screen I am on"; the claim lives on the window |

Naming the window answers three questions with one field. *Which monitor:* the
one the window most overlaps, via the same `work_bounds_for` every other path
uses, so tiling and reserving can never disagree about where a pixel is. *Who
may ask:* the existing `link.resolve(window)` ownership check, unchanged. *For
how long:* the window's lifetime — a panel that crashes cannot leave a permanent
strip carved out of the desktop, because destroying the window destroys the
claim. A side table would have needed explicit cleanup on destroy, on hide, on
move, and on hotplug: four places to forget.

**Cost accepted:** a panel must have a window before it can reserve, which rules
out a "reserve space first, map into it second" startup order. Panels do not
work that way — they know their own height before they know anything about the
screen.

### Decision 3 — a greedy claim is clamped to a third per edge, not refused

| Option | *What changes:* |
|---|---|
| Refuse a claim over the threshold | a panel that mismeasured its height gets an error it probably does not handle, and reserves nothing — so it draws over tiled windows instead |
| **Clamp to `MAX_RESERVED_FRACTION` = 1/3** (chosen) | the same panel gets a third of the screen and an answer that says so; the desktop is cramped but usable and the panel can be seen and fixed |
| No bound at all | a client asking for the full height leaves a zero work area: every tiled window collapses to nothing, with no visible cause and no window left to fix it from |

The realistic failure this guards against is not malice but arithmetic — a font
that measured larger than expected, a scale factor applied twice. Clamping keeps
the machine usable in that case; refusing does not, because the client that got
the number wrong is also the client that will not handle the error.

A third **per edge** rather than a bound on the total, so that two opposing
panels can take two thirds between them and a third always survives: one rule
applied four times, with the total following from it rather than being a second
rule to keep consistent.

### Decision 4 — zero is the release, and the reply carries the work area

`size == 0` releases the reservation. There is deliberately no separate release
request: a panel has one code path for "how much do I need" (it sends the
number, which is sometimes zero) and the compositor has one place to re-tile
from. A second request would have been a second place for the two to fall out of
step, and the state it would manage — "reserved nothing" — is already
representable.

The reply is `WorkArea`, not `Ok`. A panel needs the resulting rectangle to
place itself, and after decision 3 the granted amount need not be the requested
amount. Answering with the area also makes the honest thing the easy thing: the
area accounts for *other* panels on the same monitor, which the caller could not
have derived from its own request. A follow-up query instead of a reply would be
answerable differently by an intervening hotplug.

### Decision 5 — a hidden or minimized panel reserves nothing

A strip is kept clear so that what sits in it stays visible. Nothing is sitting
in it while the panel is off screen, so holding the strip would shrink the
desktop with no visible cause — precisely the confusing failure decision 3
exists to bound. An auto-hiding taskbar therefore gets the tiling behaviour it
wants for free: hide the window, the desktop grows.

**Cost accepted:** a panel that hides and shows rapidly re-tiles every tiled
window on its monitor each time. Re-tiling is arithmetic over a short list, and
the alternative — a hysteresis timer — is state that can be wrong.

### If this is never revisited

The shape that would force a rethink is a panel that is **not** a full-width
band on **one** monitor: a dock spanning two screens, or two docks sharing an
edge side by side. Both are expressible as extensions (a span-taking request; a
claim naming several windows) rather than as a change to this one, because the
thickness-per-edge model composes by addition and addition does not have to be
undone to be extended.
