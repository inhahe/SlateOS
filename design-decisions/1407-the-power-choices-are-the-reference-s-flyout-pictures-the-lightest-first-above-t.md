## 1407. The power choices are the reference's flyout: pictures, the lightest first, above the caret

**Date:** 2026-09-26 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The start menu's power choices were a list of words, "Shut down"
at the top, hanging off the left end of the power button. They are drawn now as
the Aero reference draws them: each with its picture, the session's choices
first and then the machine's from the lightest to switching it off -- log out,
lock, sleep, hibernate, restart, shut down -- rising above the caret that opens
them, with their right edge on the button's. "Shut down" is last, so it sits
nearest the button, whose main part already does it in one click (§1405).

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| Order | log out, lock, sleep, hibernate, restart, shut down | shut down first, as it was | the reference's `SM_POWER`: the session's choices, then the machine's, lightest first |
| Lock | second, after log out | left out, as the reference has no lock | every desktop's power menu has it, and it is the one choice that leaves everything as it was; it takes the place of the reference's "Sleep the display", the nearest thing to it |
| "Sleep the display", "Restart OS -- keep the computer on" | not offered | offered | nothing carries them out: no program turns the screen off alone, and a restart that keeps the computer on needs the kernel (`requests/c-ab-a-restart-that-keeps-the-computer-on.md`) |
| Pictures | the freedesktop names (`system-log-out` new in the built-in set) | words alone | a theme that draws those names draws these, and the built-in set draws all six |
| Where it opens | right edge on the button's, over the caret, 236 wide | left edge on the button's, 170 wide | the caret opens it, and the reference's `right: 0` and `width: 236px` |
| The rows | inside the popup's padding on every side | the full width | a lit row (§1406) is then a wash within the panel, as the reference's items are |

The login screen's power menu is its own list and is not changed.
