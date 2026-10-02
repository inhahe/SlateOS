## C-NETMANAGER-HAD-NO-WINDOW-AND-NO-EVENT-HANDLER-AT-ALL (lane C, 2026-08-26) — ✅ FIXED 2026-08-26

**In short:** the Network Manager — the program you open to pick a Wi-Fi
network, set a static IP address, reorder your DNS servers or switch a VPN on —
never opened a window, so none of it ran. `fn main` was literally `fn main() {}`.
Worse than the other unwired apps in this batch: those at least had a
`handle_click` sitting unreachable, whereas netmanager had **no event handler of
any kind** — eight tabs' worth of buttons, a Wi-Fi list, four editable text
fields and a DNS priority list existed only as pixels, with nothing anywhere in
the program that could have turned a click into an action. It now opens a
window, and every control on all eight tabs works, with a keyboard and a mouse
wheel. Two real bugs fell out of writing the tests: the Profiles tab hid the
only button that can create your first profile, and a Wi-Fi selection could
silently move to a *different* network across a scan, so Connect would have
joined the wrong one.

### What changed

1. **The window.** `apps/netmanager` gained `oswindow` and an
   `impl oswindow::app::App for NetManagerApp`, and `fn main` is now
   `app::launch("netmanager", &mut app)`.

2. **An interaction layer, built from nothing.** `Target` (21 variants) names
   every control in the program; `activate(Target)` is the one place an action
   happens; `handle_click`, `handle_key`, `handle_event` route to it. There was
   no prior version of any of this to extend.

3. **The geometry has exactly one copy, by construction.** The other apps in
   this batch got a `…Layout` struct measured once and read by both the renderer
   and the hit-test. That does not scale to netmanager: it is ~1200 lines of
   running-`y` rendering across eight tabs, and a `Layout` for it would be a
   second 1200-line transcription of the same arithmetic — which is precisely
   the failure mode the systray entry above describes, just bigger. Instead the
   renderer *records* each clickable box as it paints it (`frame.hit(target,
   rect)`), and the hit-test runs the renderer and reads `Frame::hits` back to
   front so the topmost control wins. The drawing helpers
   (`render_button`, `render_mini_button`, `render_toggle_row`,
   `render_editable_field`) return the rect they drew, so the call site can only
   register the box that was actually painted. There is no second copy to
   disagree with.

4. **A keyboard, with a visible caret.** Tab walks the IP/mask/gateway fields,
   typing lands in the focused one, Backspace deletes, Enter in the DNS box adds
   the address without reaching for the button, Escape puts the caret away (and,
   with no caret out, closes the window). The caret is drawn *into the field's
   text* rather than as a separate command, because the render tree is the only
   thing a test can see — a focus state that leaves no mark in the output is a
   focus state no test can assert.

5. **A wheel and a resize.** The sidebar scrolls through
   `guitk::wheel::Accumulator`, which keeps the fraction of a row a trackpad
   sends; rounding per event would discard slow scrolling entirely. `render`
   believes the width and height it is handed and stores them, because the
   first frame goes out *before* any `Event::Resize` — so `render` is the only
   place the true size is known on frame one, and a hit-test that trusted the
   constant would be wrong for exactly one frame at every startup.

### The two product bugs the tests found

- **The Profiles tab hid its own Add button when there were no profiles.** The
  empty-list branch printed "No profiles" and `return`ed, which skipped the
  Add-Profile button below it — so the control that creates your first profile
  was unreachable in precisely the state that needs it. It now falls through.
  Caught by `add_profile_is_reachable_when_there_are_no_profiles_yet`.

- **A Wi-Fi selection was tracked by list index across a rescan.** `refresh()`
  rebuilt the network list and merely clamped the stored index into range. A
  scan reorders the air — signal strengths change, networks appear and vanish —
  so the surviving index quietly pointed at a *different* network, and Connect
  would have joined that one. The selection now follows its **SSID**: if the
  chosen network is no longer on the air, neither is the selection. Caught by
  `a_wifi_selection_follows_its_network_across_a_scan`, which failed on the
  first run; the fix went into the production code, not the test.

### How the tests were written so they could fail

136 tests, all green. Nothing in the geometry tests recomputes a position from a
layout constant — a test that recomputes geometry agrees with the renderer only
by accident. Instead:

- **`rect_of(app, target)` reads the rect out of the rendered `Frame`**, and
  `click(app, target)` clicks its centre, so a test that clicks a button clicks
  wherever the renderer actually put it.
- **Boundaries are asserted, not assumed.** `Rect::contains` is half-open on
  both axes, so adjacent rows cannot both claim a boundary pixel;
  `the_far_edge_of_a_row_belongs_to_the_row_below_it` pins that down by clicking
  `x + w - 0.5`, never `x + w`.
- **`no_two_controls_claim_the_same_pixel_on_any_tab`** walks every tab and
  checks every pair of recorded rects for overlap, which is the assertion that
  makes "the topmost wins" a design rather than a coincidence.
- **`render_believes_the_size_it_is_handed_on_the_very_first_frame`** and
  `a_resize_is_believed_and_the_hit_test_follows_the_window` cover the frame-one
  case above, which no amount of clicking at the default size would reach.
