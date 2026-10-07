## 1492. The login screen's accessibility menu switches for the sitting, and Super+U opens it

**Date:** 2026-10-06 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the login screen has always drawn an accessibility button,
and pressing it opened a menu that was never drawn: nothing appeared, and
the next press anywhere was spent closing what nobody could see. Beside
it, a button for an on-screen keyboard -- which the system does not have
-- did nothing at all. The accessibility menu is now real: it offers the
one accessibility setting the desktop can apply that matters before
anyone signs in, high contrast, as a switch; it lasts while the login
screen is up and is saved nowhere; and Super+U opens it from the
keyboard, since the people who need it may not be able to use a
pointer. The keyboard button is not drawn while there is no keyboard to
show.

**Where:** `gui/desktop/src/login_screen.rs` -- `LoginAccess`,
`ACCESS_MENU`, `LoginAction::Access`, `a11y_menu_rect`,
`key_a11y_menu`, `render_a11y_menu`, `LoginConfig::show_osk_button`;
`gui/desktop/src/session.rs` -- `login_high_contrast`, `paint_login`,
`apply_login_action`.

| Choice | Instead of | For | Against |
|---|---|---|---|
| **High contrast alone in the menu, more as they exist** | also "larger text" and "reduce motion" | The login screen draws at fixed sizes and its only motion is a shake after a wrong password, so those switches would change nothing on it: a menu of switches that do nothing is the defect this replaces. High contrast changes every colour it draws, through the palette it already takes. | A menu of one row; text size waits on the screen laying out at the user's size. |
| **For the sitting, saved nowhere** | saving it as the machine's login appearance | Nobody has signed in to own a setting, and writing a machine-wide file from an unauthenticated screen is a write any passer-by can make. Once someone signs in, the session has their own settings -- which is what Windows' sign-in screen does with its own switches. | Someone who always needs it switches it at every sign-in until a setting for the login screen exists (a page lane E would own). |
| **Super+U opens it, from anywhere on the screen** | Tab walking to the bar's buttons | The screen has no focus order to walk -- its keys are the account list's and the password field's -- and building one is a larger change than this one; Super+U is the chord other systems' accessibility settings answer, and does not type into the field. | One more chord to learn. (The power menu was still pointer-only here; §1493 gives the bar a keyboard path.) |
| **The menu stays up after a switch** | closing it, as the power menu closes after a choice | A switch is not a choice: the user sees it switched and can switch it back; a choice of shutting down is done. | Escape, or a press beside it, to close it. |
| **No keyboard button while there is no keyboard** | drawing it, as before | A button with nothing behind it is a click that does nothing, in the one place a user who needs one cannot go elsewhere to look. | When an on-screen keyboard exists, `show_osk_button` must be turned on with it. |

**Revisit when:** a screen reader or an on-screen keyboard exists (each
earns a row, or the button back), or a settings page for the login screen
does (then the switch could start from it).
