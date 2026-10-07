## 1493. Tab walks the login screen's middle and its bar, and typing on the bar types into the field

**Date:** 2026-10-06 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the login screen's power and accessibility buttons could be
reached only with a pointer, so someone using the keyboard alone could
not shut the machine down or restart it from there. Tab now walks from
the middle of the screen -- the accounts or the password field -- to the
accessibility button, then the power button, and round again (Shift+Tab
backwards); Enter or Space on a button opens its menu, and the power
menu's choices are walked with Up and Down and chosen with Enter. A
ring marks the button the keyboard is on. Typing while the keyboard is
on a button goes back to the password field and types there, so a
password begun on the wrong control is not lost.

**Where:** `gui/desktop/src/login_screen.rs` -- `LoginFocus`,
`tab_stops`, `step_focus`, `key_bar`, `key_power_menu`,
`render_bar_focus`; `gui/desktop/src/login_screen/accessible.rs` says
where the keyboard is.

| Choice | Instead of | For | Against |
|---|---|---|---|
| **Tab walks three places: the middle, the accessibility button, the power button** | a stop for every control (each account, the eye, Sign In, the arrow back) | The middle already has its own keys -- arrows walk the accounts, Enter signs in, Escape goes back -- so three stops reach everything the keys could not, without making the common path (type, Enter) longer. | The eye is still a pointer's alone; a stop for it is one more line in `tab_stops` when someone needs it. |
| **Typing on a button goes back to the field and types** | ignoring it, as a button takes no text | A user who tabbed away and starts typing their password would otherwise lose the first characters without a sign; the field is the only place text means anything here. | A letter is never a button's shortcut on this screen -- and could not become one without undoing this. |
| **The power menu walked by its keys only when opened from the keyboard** | always | A menu opened by the pointer keeps answering only Escape, as it did; a keyboard ring that appeared on a menu the pointer opened would mark a row nobody chose. | Two ways for the same menu to behave, by how it was opened. |

**Supersedes** the "the power menu is still pointer-only" in §1492's
Super+U row.
