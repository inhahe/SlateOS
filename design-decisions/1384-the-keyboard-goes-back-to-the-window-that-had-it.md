## 1384. The keyboard goes back to the window that had it, by one rule for every way a window leaves the screen

**Date:** 2026-10-10
**Lane:** F
**Decided by:** Claude (autonomous), for the operator's rule §1242 (a
consent prompt is answered only on purpose), whose request asks that "the
keyboard goes back to the window that had it before".

**In short:** when the window you are typing in closes, is minimised,
hidden or moved to another desktop, or you switch desktops, the keyboard has
to go somewhere. The compositor had four copies of that choice, and they
disagreed. Closing a window gave the keyboard to the topmost window in its
own band (the ordinary windows' band, not the taskbar's). Minimising or
hiding gave it to the topmost window of any kind -- usually the taskbar --
and never told the minimised window it had lost the keyboard, so the window
list could show it focused beside the window that was. Switching to a
desktop with no window gave it to the taskbar. Now one rule decides, for
all of them: the keyboard goes back to the window you used most recently
that can take it, in the same band or below.

### The rule

| Question | Decided | Alternative | Why |
|---|---|---|---|
| Which window | The one that held the keyboard most recently, still on screen | The topmost (what closing did) | The topmost is not where the user was: a window kept always on top by a rule, or one opened in front of theirs, sits above their window without their having used it. KDE's and GNOME's window managers keep exactly this list (KWin's "focus chain", Mutter's most-recently-used list) for exactly this choice. |
| Which band | The departing window's band or below | Any band (what minimising, hiding and switching desktops did) | The taskbar is in front of every application by construction, and was handed the keyboard whenever an application was minimised. Closing already had this rule, with a test for it. |
| When no window qualifies | The topmost window in the band or below; else none | -- | A desktop whose windows never held the keyboard (just opened by the shell) still gets one; a desktop with no application focuses nothing, the taskbar included. |
| A refused focus | Changes nothing | Told the window with the keyboard it lost it, and left it there | The compositor and the client then disagreed about where keys went. |
| An accent half typed (a dead key) | Disarmed whenever the window that holds the keyboard loses it, closing included | Disarmed only on a focus change from one window to another | Closing passed the keyboard on with no window to take it *from*, so an accent armed in a closed window completed itself in the next. |

The history (`Compositor::focus_history`) holds each living window once, in
the order they last held the keyboard, and a window leaves it when it
closes, so it is bounded by the window count.

### What it is for, beyond tidiness

§1242's prompt opens over the window the user is typing in; when the user
moves the keyboard into it and then answers, the keyboard must go back to
the window it came from. The prompt is in the shell's band (in front of
every application), so "topmost in the band or below" would have given the
keyboard to whatever happened to be on top. With this rule it goes back to
the window the user left -- which is also what the rest of the request
builds on: a window that opens without taking the keyboard, and a way for
the shell to give it back (`requests/e-cf-a-consent-prompt-is-answered-only-on-purpose.md`,
lane F's part).

### Revisit if

A window should take the keyboard back on its own: a dialog, closed, giving
it to its parent rather than to the most recent window. Today a dialog's
parent is usually the window used just before it, so the two agree.
