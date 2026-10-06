## TD-C-AN-OPEN-MENU-FROSTS-THE-WHOLE-SCREEN (lane C, 2026-10-06) — OPEN

**Status:** OPEN -- found 2026-10-06 by the theme preview; the fix below
(the second way) is lane C's next.

**In short:** while any of the shell's menus is open -- the start menu, the
calendar, a right-click menu -- the whole screen behind it is blurred and
tinted, the taskbar included, not just the part behind the menu. A menu is
meant to be a pane of frosted glass over the desktop; instead the desktop
goes out of focus round it.

**Found by** the theme preview (`gui/themepreview`), whose first picture
showed it: drawn as the session draws it, everything but the start menu was
frosted.

**Where:** `gui/desktop/src/session.rs`, `ShellSession::start_with`: the
popups surface ("Shell menus") is the size of the screen -- so a press
anywhere can reach the shell and close what is open -- and asks for
`BlurKind::Menu` behind it. The compositor blurs behind a surface's whole
frame (`Compositor::blur_behind_window`, `win.frame_rect()`), so the whole
screen is blurred and tinted under it.

**To reproduce:** open the start menu; everything outside it is frosted. Or
draw the scene with the popups surface as the session makes it.

**The fix:** the glass where the menus are, and only there. Two ways, the
second lane C's alone:

1. A surface says where its glass is -- a list of rectangles the compositor
   blurs behind instead of the whole frame. A protocol change (lane F's).
2. The session keeps the whole-screen surface for the menus' drawing and
   their presses, asks it for no blur, and puts an empty surface that takes
   no input under each open menu's box, asking for the menu's glass there --
   moved and sized as menus open and close, as the notifications' surface
   already is. The theme preview draws its start menu this way.

**If never fixed:** every menu opening frosts the desktop, which reads as the
desktop losing focus; and on a software compositor it costs a blur of the
whole screen per frame while a menu is open.
