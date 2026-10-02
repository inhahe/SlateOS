## TD-C-A-ROTATING-WALLPAPER-NEVER-TURNED-ON-A-DESKTOP-NOBODY-TOUCHED (lane C, 2026-09-26) -- FIXED the same day

**Status:** FIXED 2026-09-26 -- `WallpaperManager::next_change_in` is one of
the times `ShellSession::arm_next_frame` sleeps until.

**In short:** a wallpaper rotation (a folder of pictures, changing every so
often) moved only when something else happened to wake the desktop -- an
animation, a clock widget, the pointer over the taskbar. On a desktop nobody
was touching, which is when a rotation is looked at, it showed its first
picture for ever. A dynamic (time-of-day) wallpaper likewise.

**Why.** The shell's loop sleeps with no bound when nothing is due -- the
property `design-decisions.md` 812 defends -- and wakes at the soonest of the
moments it knows about: an animation frame, a widget clock, quiet hours, the
automatic light/dark edge. The wallpaper's next picture was not one of them;
its `tick` ran in `step_frame`, on frames armed for other reasons.

**Three smaller faults on the same path, fixed with it:**
- The wallpaper was ticked with the clock from *before* the frame's time was
  added, so the frame the loop was woken for, at the moment the picture was
  due, saw the moment before it: a wake-up late.
- The slideshow timer used 0 for "not started", and the shell's clock starts
  at 0: a picture put up in the first second had its interval started again.
  It is an `Option` now.
- A folder of one picture "advanced" to itself each interval, issuing a new
  image id -- so the shell read and decoded the same file every time.

**And the live path.** A rotation chosen in Settings while the desktop is up
arrives as a settings announcement, which re-read the settings but armed
nothing: with the user in some application window, no pointer event reaches
the shell to re-arm it. `adopt_appearance_change` now re-arms when nothing is
animating -- which also covers switching to the automatic light/dark mode
live, whose first edge had the same gap.

**Tests:** `an_idle_desktop_is_woken_for_the_slideshows_next_picture` goes
through the wake-up: one frame, the armed delay, one frame of that length, the
next picture uploaded and drawn. With the dynamic wallpaper's minute, the
rotation chosen live, and the manager's `next_change_in` cases.
