### [F] Pointer input reached the wrong window four ways, and no left-button release reached any window at all -- 2026-09-24

**Status:** ✅ FIXED 2026-09-24 (lane F) — `gui/compositor/src/lib.rs`: `pointer_target`, `track_pointer_window`, `press_in`, and the release path of `handle_mouse_button`.

**In short:** clicking a checkbox with the mouse did nothing and a clicked
button stayed drawn pressed, because the compositor never passed on the
release of the left button — to any window, ever. Found while fixing the
narrower bug this entry was opened for (hover highlights that stayed lit), in
the same forty lines of input routing, alongside two more.

| fault | what a user saw |
|---|---|
| **A left-button release never reached a window.** `handle_mouse_button` returned on every left release, whether or not it had just ended a window drag. The toolkit's checkbox toggles on the release, its button un-presses on it, and 73 sites across `gui/toolkit` and `apps/` read it. | checkboxes could not be ticked with the mouse; buttons stayed drawn pressed; drags inside applications (sliders, selections, the colour picker) never ended |
| **No `Enter`/`Leave`.** The protocol carries both and a dozen sites consume them; the compositor emitted neither. | a hover highlight stayed lit after the pointer moved to another window |
| **A title bar did not hide the window beneath it from the pointer.** `window_at` looked for the topmost *client area* containing the point, so a point on one window's title bar fell through to the client area of whatever was beneath it. | moving, scrolling or right-clicking on a title bar acted on the window underneath |
| **No implicit grab, and releases went to the focused window.** Motion went to whatever window was under the pointer, and a non-left release to the focused window. | a slider or text selection dragged past the window's edge stopped following; a right-click on an unfocused window had its release sent to a different window |

**The fix.** One hit test, `pointer_target` — client area, frame, or desktop,
topmost first, skipping click-through windows — behind motion, scroll, button
and cursor-shape routing alike. `track_pointer_window` sends `Leave` and `Enter`
as the client area under the pointer changes, including when the pointer leaves
the output. A press in a client area grabs the pointer for that window until the
button is released (`press_in`); motion, scroll and the release go to it
wherever the pointer is, and a release whose press no window saw goes nowhere.
A closed window drops out of both.

**What holds it.** One test per fault, each checked to fail with the old code
restored: `a_left_click_reaches_the_window_as_a_press_and_a_release`,
`the_pointer_moving_between_windows_leaves_one_and_enters_the_other`,
`a_title_bar_hides_the_window_beneath_it_from_the_pointer`,
`a_drag_keeps_the_pointer_in_the_window_it_started_in`,
`a_right_click_releases_to_the_window_it_pressed_in`, plus
`a_release_with_no_press_behind_it_goes_nowhere`,
`the_pointer_leaving_the_output_leaves_the_window_it_was_in`,
`a_click_through_window_takes_neither_the_pointer_nor_its_shape` and
`a_closed_window_no_longer_holds_the_pointer`.

**Why none of it was caught.** Every routing test asserted on the press. A
click is two events, and the half nobody checked was the half that did not
arrive — the same shape as a test that asserts the part of a feature that
works.

**Left as it is, on purpose:** a drag that leaves the *development host's*
window ends the grab, because the host sends `WM_MOUSELEAVE` and the release
happens where the compositor cannot see it. On a real display the pointer
cannot leave the output.
