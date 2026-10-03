## 1419. What Alt+Tab shows is what Alt+Tab is bound to

**Date:** 2026-09-27 &middot; **Decided by:** Claude (operator-approved scope: the
operator asked for "which behavior Alt+Tab uses" in the settings, §1416; the
shape of it is Claude's) &middot; **Lane:** C

**In short:** Alt+Tab can now show the overview -- every window on the desktop,
to scale -- instead of the small strip of pictures, which the operator asked
for when Super+Tab went. It is not a separate setting. There is a second
window-switching action, "Cycle Windows in the Overview", and a user who wants
the overview binds Alt+Tab to it on the keyboard shortcut card, the way any
shortcut is changed. It behaves as Alt+Tab does: hold Alt, Tab through the
windows, let go to pick.

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| Where the choice lives | which action the chord is bound to, on the shortcut card | a setting, "Alt+Tab shows: switcher / overview", in a settings file with its own control | one place says what a chord does; a setting beside the binding would be a second answer to "what does Alt+Tab do", and a user could bind Super+Tab to the overview and keep Alt+Tab as the strip, which a single setting cannot say |
| What the overview does during a switch | Alt+Tab's manners: Tab and Shift+Tab step, the arrows and the pointer move the lit card, letting go of Alt takes it, Escape leaves | the overview's own: a toggle, the chord opening it and pressing it again closing it | a toggle on Alt+Tab would make the second Tab close the overview rather than move on, which is not what anybody holding Alt means |
| The cards' order during a switch | most recently used first, left to right, so each Tab moves one card on | the window list's order | the lit card would otherwise jump about the grid |
| Shift+Alt+Tab | shows its switch the way the same keys without Shift do | always the strip | binding Alt+Tab to the overview would otherwise leave its reverse opening the strip |

**What turned up in the doing, and was fixed with it:**
- **The switcher went the wrong way after the first Tab.** It counted through
  the taskbar's bottom-to-top order, so the first Alt+Tab reached the window
  before this one but the second went back to the window being left and the
  third to the oldest. It counts most recent first now, as every desktop does;
  Shift+Alt+Tab from nothing reaches the window used longest ago (with two
  windows it used to land on the one already in front).
- **Only Alt's release ended a switch.** With window switching bound to
  Super+Tab, the switcher opened and nothing closed it. A switch now ends when
  a modifier of the chord that started it comes up -- Shift excepted, which is
  the direction, not the hold -- or, for a bare key, when that key comes up.
- **The overview's arrow keys walked every desktop's windows** in its
  one-desktop view, so an arrow could light a card that was not on the screen.
  They walk the drawn cards, in the drawn order.
