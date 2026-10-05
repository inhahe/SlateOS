## 1413. Alt+Tab is the reference's glass: each window's picture in a cell, the chosen one's title across the top

**Date:** 2026-09-27 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The window switcher was a box 400 pixels wide with the first
twelve characters of every window's title in a row, cut with no mark -- so
two documents of one program read alike, a long title read as a short one,
and twenty windows ran off both ends. It is drawn in the start menu's glass
now: each window's program picture in a cell of its own, the window the
switch would go to marked as the accent marks the keyboard's row in the start
menu, and that window's title -- whole, or cut with a "..." -- across the top.
A long list wraps into rows, and past what the screen holds turns pages, so
the chosen window is always on screen.

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| What each window shows | its program's picture | a live picture of the window, as Windows 7 draws it | the compositor has no way yet to give the shell a window's contents (the same gap keeps the taskbar's previews waiting); the program picture is what the taskbar tile already shows, so a window looks the same in both |
| Which titles | only the chosen window's, whole | every window's, each cut to its cell | a cell is too narrow for any real title, and the one title that matters is where the switch goes |
| The glass | the start menu's, through two shared halves (`glass_under`, `glass_edge`) | its own fill and an accent outline, as before | the reference has no switcher; a floating panel of the desktop's is its glass, and one implementation keeps the two alike |
| Too many windows | pages of whole rows, turned by the selection | a scrolling strip | a page never shows a cell cut in half, and the chosen window is on every page that is shown |
