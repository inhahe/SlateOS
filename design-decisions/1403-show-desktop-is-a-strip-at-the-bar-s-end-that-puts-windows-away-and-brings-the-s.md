## 1403. "Show desktop" is a strip at the bar's end that puts windows away and brings the same ones back

**Date:** 2026-09-26 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The taskbar ends, right of the clock, in the Aero reference's
narrow "Show desktop" strip. A press puts away every window on the desktop
being shown; the next press brings back exactly those windows, the one that was
in front in front again -- unless a window has been shown in between, in which
case the desktop is no longer what was shown and the press puts windows away
again. The Show Desktop shortcut is the same switch.

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| Where | the very end of the bar, 14 pixels, right of the clock | a tray icon | the reference's `aero-showdesktop`; a pointer thrown into the corner lands on it |
| The second press | brings back what the first put away, bottom of the stack first | puts away again (a one-way action) | every taskbar's corner toggles; the order puts the window that was in front back in front |
| Bringing back | `Activate` each | `Restore` | `Restore` also un-maximises; a maximised window put away should come back maximised |
| When the toggle forgets | the moment any window is shown again on this desktop, by any means | never; or after a timeout | a window shown in between means the desktop is no longer what was shown, and bringing back the rest over it would undo the user's choice |

### Not done here

- **Aero Peek** -- the reference's hover preview of the desktop through
  transparent windows needs the compositor to draw windows see-through on
  request, which it cannot yet.
