## 513. A window is never placed where it cannot be reached, and "reachable" is a question about the whole desktop

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** §512 stopped a *resize* from leaving a window off the screen. It
did not stop the two other ways that happens: un-maximising a window puts it
back at the rectangle it had before it was maximised, and if the screen changed
size in between, that rectangle may be nowhere. §512 also, on its own, contained
a bug of exactly the kind it existed to prevent — with two monitors it asked
"is this window on the screen that just resized?", so shrinking the first
monitor dragged every window off the second one and onto the first.

**Context.** A window has two saved rectangles: `restore_rect`, written when it
is maximised or snapped, and `fs_restore_rect`, written when it goes fullscreen.
Both are recorded against the desktop *as it was at that moment*, and both are
**consumed** by the restore — `take()`n out of the window. So a bad restore has
no second chance: after it, nothing in the compositor knows the window was ever
anywhere else. `resize_display`'s rescue does not help, because it runs at resize
time, when the window is still maximised and therefore not stranded at all; what
is stranded is a rectangle in a field.

Three reachable failures:

1. Maximise a window near the bottom-right, change resolution downward,
   un-maximise. The window lands entirely off the screen. This is the ordinary
   "I plugged in a projector" sequence.
2. The same with fullscreen — worse in practice, because a game is exactly the
   sort of program that changes the resolution while it is fullscreen.
3. Two monitors, resize the first. Every window on the second is yanked onto the
   first, because `bring_stranded_windows_back` tested against the bounds of the
   display that changed rather than against the desktop.

**Decision.** One helper, `kept_reachable(frame, desktop, fallback)`, used by all
three sites: the frame is returned unchanged if any part of it falls on
`desktop`, and otherwise pulled onto `fallback` by `pulled_onto`.

**Why two rectangles rather than one.** The two questions are genuinely
different, and collapsing them is precisely what caused failure 3. *Can the user
see this window?* is asked of the whole virtual desktop — a window on the second
monitor is not stranded because the first shrank. *Where does a window nobody can
see go?* has to name a screen that actually exists, and the union of several
monitors is not necessarily one: an L-shaped arrangement has a hole in its
bounding box, and a window dropped in the hole is as lost as it was. So the test
is against the union and the destination is a single real screen.

**Which screen is the fallback.** For the resize rescue it is the display that
just changed — a screen known to exist, and the one most likely to have stranded
the window. For a restore it is the display the window is on *right now*, which
is well-defined precisely because a window being restored is currently maximised
or fullscreen and therefore demonstrably covering a real monitor. The alternative
— always the primary — is simpler and wrong: un-maximising a window on the second
monitor would teleport it to the first, which is the same class of bug as failure
3, just triggered by a different action.

**Strictness is unchanged from §512**, deliberately: *no part of the frame on any
screen*. A restore rectangle hanging off an edge is still visible and still has a
title bar to grab, so it is used exactly as saved. That rule now has its own test
and its own reintroduction, because it is the rule most likely to be "improved"
later into a tidy-everything-up pass.

**A supporting change:** `Window::frame_rect` is now the special case of a new
`frame_rect_for_client(client)`. Every reachability question is a question about
the *decorated* box — it is the title bar that must be on screen to be grabbed —
but the saved rectangles are client geometry, and they belong to a window that is
not currently at them. `frame_rect` cannot answer that; the general form can, and
stating the inset arithmetic once keeps it the exact inverse of
`client_geometry_for_frame`.

**Verification.** 8 new tests, 8 reintroductions. Four of the tests have a
reintroduction that breaks them and nothing else: the multi-monitor evacuation,
the fullscreen restore, the hanging-off-an-edge guard, and the fallback-screen
choice. Three of the remaining are broken only by `restoreframeisclient` — a
marker that also breaks five *pre-existing* restore tests, so those three are
additional coverage of an invariant that was already guarded rather than the sole
guard on a new one. That is stated rather than glossed: a test whose only proof
is a marker that breaks eight tests has not been shown to be load-bearing, and
pretending otherwise is how a suite fills up with tests nobody can delete because
nobody knows what they cover.
