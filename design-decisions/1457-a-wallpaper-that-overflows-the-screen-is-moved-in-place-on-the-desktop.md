## 1457. A wallpaper that overflows the screen is moved in place, on the desktop

**Date:** 2026-10-01 &middot; **Decided by:** Claude (autonomous) &middot;
**Lane:** C

**In short:** A picture that fills the screen usually spills off two of its
sides, and the desktop showed its middle, always. Now the user chooses
which part shows: right-click the desktop, "Move wallpaper", and drag the
picture (or use the wheel or the arrow keys) until the part they want is
on screen; Enter keeps it, Escape puts it back. The login screen, showing
the same picture, shows the same part. `design.txt`: "let the user scroll
the image up/down or right/left to center it on the desktop how they
want".

**How it works:** `AppearanceSettings::wallpaper_position`
(`wallpaper.position_x`, `position_y`): fractions of the room the picture
leaves, `fit_image`'s `align`, so 0 shows its left or top edge, 0.5 its
middle (the default, and what was always shown), 1 its right or bottom
edge. The session tells the shell how much room the picture has
(`DesktopShell::wallpaper_room`) whenever it paints the background, and
draws whatever position the shell holds, so a drag is seen as it is made.

**Choices:**

| Choice | Taken | Alternative | Why |
|---|---|---|---|
| Where the user moves it | on the desktop itself, from its menu | only in the Settings app, on a preview | the desktop is the picture at its real size and place; a preview is a guess at both |
| What is stored | fractions of the room | a pixel offset | a fraction survives a change of screen size or picture: the same part stays chosen; pixels would point somewhere else on the next monitor |
| When it is offered | only where the picture has room (Fill, Fit or Center, and not a picture that fits the screen exactly) | always | an item that does nothing is a door to nowhere |
| Ending it | Enter keeps, Escape puts back, a press on the taskbar or another popup opening keeps | anything but Enter puts back | the picture moved is what the user sees and was making; only Escape says to undo it |
| The fit for a folder of pictures | applied, as for one picture (a bug, fixed with this) | -- | the folder kept whatever fit was in force when it started |
