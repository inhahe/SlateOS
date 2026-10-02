## TD-C-THE-DESKTOP-BACKGROUND-WAS-PAINTED-ONCE-AND-NEVER-AGAIN (lane C, 2026-09-26) -- FIXED the same day

**Status:** FIXED 2026-09-26 -- `ShellSession::refresh_background` sends the
background whenever its frame differs from the one last sent.

**In short:** the desktop's background surface -- the wallpaper, the icons on
it and the widgets over them -- was drawn when the shell started and when the
display changed size, and at no other time. An icon clicked was selected where
nobody could see it; an icon or widget dragged stayed drawn where it had been;
a clock widget showed the minute it was added; a wallpaper, a fit or a theme
chosen in Settings appeared only after a restart or a change of resolution; a
slideshow never showed its next picture.

**Why it lasted.** Each of those is a change to the shell's model, and each
test of it read the model -- `selected_ids()`, `render_widgets()`,
`current_image_path()`, the clock's wake-up -- one step short of the screen.
The session's own test was named for the design,
`the_background_is_painted_once_and_the_chrome_on_every_change`, and
`paint_background` gave the reason: "the one surface whose picture does not
depend on anything an input event changes". That was true while the
background was only the wallpaper. The widgets and then the icons (2026-09-14)
joined it without anything telling the pump, which repainted the chrome alone.
It is `TD-C-A-PURE-FUNCTIONS-TESTS-SAY-NOTHING-ABOUT-ITS-CALLER` one level up:
the parts were right and the door to the screen was shut.

**The fix.** Every dirty pump builds the background's frame and sends it only
if it differs from the last one sent (`RenderCommand` and `RenderTree` are now
`PartialEq`). Compared rather than flagged per cause, because the causes are
spread over the icon layer, the widget manager and the wallpaper, and a flag is
one more place each new cause must be taught about -- the first one forgotten
is this bug again, with every test green. `paint_background` stays the paint
that always sends. The tests drive the real input path and assert on the frame
sent: an icon clicked, a widget dragged, a wallpaper chosen in Settings; and a
taskbar click and an idle pump send nothing.
