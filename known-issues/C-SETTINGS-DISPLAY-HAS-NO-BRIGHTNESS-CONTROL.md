## `C-SETTINGS-DISPLAY-HAS-NO-BRIGHTNESS-CONTROL`

**In short:** the Settings app's Display page offers resolution, refresh rate,
scale, orientation and a night-light section, but no way to change screen
brightness. It is the one display setting a laptop user reaches for most, and
it is simply absent — not broken, not greyed out, not there.

**Where:** `apps/settings/src/main.rs`, `build_display_page`. Nothing in
`apps/settings/src/` mentions brightness at all, and `SettingsState` has no
field for it. The entry `C-SETTINGS-SLIDERS-CANNOT-BE-DRAGGED` listed
"brightness" among the sliders that could not be dragged, which was wrong —
there was never a brightness slider to drag.

**How to confirm:** open Settings → Display and look for it.

**The proper fix:** two halves, and only the first is ours.

1. *The control.* A `brightness_percent: u8` on `SettingsState`, a
   `SliderId::Brightness` with range `(0.0, 100.0)`, and one
   `self.slider(s, "Brightness", SliderId::Brightness)` in `build_display_page`.
   With §476's plumbing in place that is a handful of lines, and the existing
   slider tests cover it the moment the id joins `SliderId::FIXED`.
2. *Somewhere for it to go.* A slider that moves a number in a settings process
   and changes nothing on screen is worse than no slider, because it claims to
   work. Real brightness needs a backlight control path — DPMS/DDC-CI over the
   display connection for external monitors, a panel backlight interface for
   built-in ones — which lives behind the compositor and does not exist yet.
   See `TD-COMPOSITOR-HAS-NO-SCANOUT`.

So this is deliberately left undone rather than half-done: adding (1) without
(2) puts an inert control on the page, which is exactly the fault
`C-SETTINGS-BUTTONS-WITH-NOTHING-BEHIND-THEM` records for seven buttons and
which that entry argues against repeating. Do (1) when (2) lands.
