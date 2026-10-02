## TD-C-A-SWEEP-FOR-CODE-THAT-ASSUMES-THE-SCREEN-IS-1920-BY-1080 -- DONE 2026-09-13

**Date:** 2026-09-13. **Lane:** C.

**In short:** three separate bugs in one day came from the same mistake — a
piece of code comparing against a fixed 1920x1080 instead of asking how big
the screen actually is. Rather than wait to meet the fourth, this is a
deliberate look through every place in this lane's tree that mentions those
numbers. Two more real defects, one false alarm, and two cases that are
correct and should stay.

**Why it was worth doing.** The failures this produces are invisible on the
machine they are written on and obvious on anyone else's, and they are all
the same shape: something is drawn or bounded past an edge the code does not
know about. On a smaller display that means rows, buttons and notifications
that cannot be reached at all.

| where | verdict |
|---|---|
| `guitk::menu` | **real, and live.** Every popup in the shell was placed against the constant. At 1024x768 the tray overflow menu was capped to a 1080px panel, put at y=0, and drawn to y=1080 — 312 pixels past the bottom, every row below 768 unreachable. Fixed |
| `guitk::menubar` | **real, latent.** Its constants said *"Matches `menu.rs`"* and did, bug included. No consumers, so the fix was free and lands before anyone meets it. Fixed |
| `desktop::notif_pane` | **real, and not where it looked.** The constant is a documented pre-first-render default and is fine. The defect was that **nothing ever called `set_screen_height`**, which the pane's own doc asks a keyboard-driven shell to call. Fixed |
| `desktop::screen_capture` | real (`effective_region` returns a fixed 1920x1080 for full-screen) but the module has no consumers *and* screen capture is blocked on a compositor capability that does not exist. Left, recorded here |
| `apps/magnifier` | correct as it stands: *"The screen this magnifier is pointed at, until there is an API to ask."* A labelled placeholder for the same missing capability |
| `apps/lockscreen` | **false alarm.** The constants are the size the window *asks* for; `render` overwrites both fields with the size it was *granted*, and says why: *"a render that kept 1920x1080 on an 800x600 window would draw the password field in one place and accept clicks for it in another"* |

**The lock screen is the row worth keeping.** It has the same two constants
with the same two values as the modules that were broken, and it is right.
A sweep that counted occurrences would have filed a bug against it; reading
it shows the design is correct and the constants are something else
entirely. This lane keeps relearning that a grep counts *shapes* and a
defect is a *thing*.

**What actually makes these findable, and it is not the number.** In all
three real cases the give-away was the same: a placement rule that had no
parameter for the screen. `menu.rs`'s `show(x, y)`, `menubar.rs`'s
`place(...)`, and `notif_pane`'s keyboard path all decided where something
went without anything in scope telling them where the edges were. The
constant was how they filled the gap. So the check that generalises is
"does this position something against a boundary it was not handed?" rather
than "does this file say 1080?".

**And why the tests could not have caught any of it.** Every test involved
used a 1920x1080 window, and every piece of code under test compared against
a 1920x1080 constant. Two readings of one number agree however wrong the
number is. Each fix therefore adds a second screen size — `SMALL` in
`menubar.rs`, an explicit 1024x768 in the others — because that, not the
fix itself, is what stops it coming back.
