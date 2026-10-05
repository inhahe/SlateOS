## 512. A display resize re-derives everything placed by a rule, and rescues only what the user could no longer reach

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** When the screen resolution changes, the desktop has to be put back
in order. A maximised window remembers the *rectangle* it was given, not the
fact that it was maximised, so on a smaller screen it keeps its old oversized
rectangle with its close button in pixels that no longer exist. The question is
how much of the desktop a resize is allowed to rearrange. The answer taken here:
rearrange everything the compositor itself placed, and of the windows the *user*
placed, move only the ones that ended up completely off the screen — and those
by the smallest amount that brings them back.

**Context.** `Compositor::resize_display` resized the framebuffer, updated the
primary display's dimensions and marked the screen fully damaged. It did nothing
else, which left four separate failures:

1. **A maximised or snapped window kept the old screen's rectangle.** This is the
   same fact `retile_for_work_area_change` already exists to handle when a
   taskbar appears or changes height — its own doc says it: *"A window that is
   maximized or snapped holds a rectangle, not the rule that produced it."* A
   mode switch is that problem at its largest.
2. **A fullscreen window kept the old framebuffer's dimensions**, because
   `set_fullscreen` sizes from `self.backend.size()` at the moment it is called
   and nothing re-asserts it afterwards.
3. **Re-tiling would have shrunk a fullscreen window**, because
   `retile_for_work_area_change` matches `maximized || snapped` and `maximized`
   stays set underneath fullscreen. A game whose window was maximised before it
   went fullscreen would visibly pull away from the screen edges when a taskbar
   appeared behind it, permanently — and, having stopped being exactly
   display-sized, would silently lose the direct-scanout bypass.
4. **A window near the old bottom-right corner ended up entirely off the new
   screen**, with no title bar on it to drag it back by and no pointer position
   that exists. The pointer itself had the same problem: it is not derived from
   anything, so a screen that shrinks under it leaves it at a coordinate the
   display no longer has, invisible and hit-testing against nothing, until the
   user moves the mouse.

**Decision.** `resize_display` runs four passes in a fixed order: re-tile,
re-fit fullscreen, rescue stranded windows, clamp the pointer. Three questions
had real answers on both sides.

**(a) How much is a resize allowed to move?** The alternative — re-flow the
whole desktop, the way a tiling window manager would — was rejected. A user who
dragged a window somewhere chose that spot; a resolution change is not a request
to undo that choice, and a desktop that scrambles itself every time a projector
is plugged in is worse than one that leaves a window half off an edge. So the
rescue test is *no part of the frame intersects the new bounds*, which is
deliberately as strict as it can be: a window hanging half off the edge is still
visible and still has a title bar to grab, so it stands; a window with nothing
at all on screen cannot be dragged, cannot be clicked, and is gone until the
display is made large again. Only the second is a rescue rather than a
re-layout. The cost of the strictness is a genuine one — a window whose title
bar is off the *top* is unreachable while three pixels of its bottom edge remain
on screen, and this does not rescue it. That is a smaller, separable bug about
title-bar reachability rather than about resizing, and pretending to fix it here
would have meant moving windows their owners can see.

**(b) Where does a window too big for the new screen go?** `pulled_onto` pins the
top-left rather than centring or pinning any other corner. It cannot make an
oversized rectangle fit — it never changes the size — so it has to choose which
part falls off, and the top-left is where the title bar is. Any other anchor
pushes the title bar off the top or left edge and leaves the window exactly as
ungrabbable as it started, which defeats the entire point of the rescue.

**(c) Does fullscreen outrank maximised?** Yes, and the re-tile now excludes
fullscreen windows outright. Fullscreen is defined against the whole *scanout
surface*, which is what earns it the direct-scanout bypass; the work area is the
screen minus the panels, which is a different rectangle by definition. The
`maximized` flag is deliberately left *set* rather than cleared, so that leaving
fullscreen still finds a tiling state to fall back to — the exclusion is in the
filter, not in the state.

**Also decided:** the pointer is clamped to the *virtual* bounds (the union of
all displays) rather than to the resized monitor, because the pointer crosses
monitors and clamping it to the primary would teleport it off a second screen
that a resize of the first did not touch. And the rescue drops the
`WindowNotFound` from `move_window` rather than propagating it: every id came
from a list built two statements earlier, so it is unreachable, and a caller
adopting a new display mode has no useful answer to "one window declined to
move" and must not abandon the rest of the resize over it.

**Consequences.** `Compositor` is now genuinely resizable after construction,
which was the missing half of `TD-COMPOSITOR-CANNOT-CHANGE-MODE`; the kernel
half (`DRM_IOCTL_MODE_SETCRTC`) is filed to lane A as
`requests/c-a-drm-setcrtc.md`, which was held back until a caller existed. The
same `pulled_onto` helper is the right fix for the neighbouring
un-maximise/un-fullscreen case, where a stored `restore_rect` can point off the
current display — that is a separate change, because it also has to cover a
monitor being unplugged, which `resize_display` never sees.

**Verification.** 12 tests, each proved to be a regression test by reintroducing
the defect it names — via the guarded reversible patcher, one at a time — and
confirming a deterministic failure that names the test back. Eleven
reintroductions were needed to cover them, including two whose only purpose is
to prove the *negative* tests: that a window still on screen is not moved, and
that a pointer already on screen is not teleported. A guard nobody can break is
not a guard.
