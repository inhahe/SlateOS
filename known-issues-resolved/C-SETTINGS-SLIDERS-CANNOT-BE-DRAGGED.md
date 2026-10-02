## `C-SETTINGS-SLIDERS-CANNOT-BE-DRAGGED` — fixed

**Fixed** (see `design-decisions.md` §476). Sliders are now press-drag-release:
`SliderId` names each one and states its range, `SettingsState::slider_fraction`
and `set_slider_fraction` are the one mapping between a stored setting and a
handle position read in each direction, and `AnchorId::Slider` asks the page
itself where the track was painted so a drag measures from the bar on screen.
`slider_track` is the single placement the painted bar, the grab band and the
drag origin all come from — the remedy `pill_rect` applies to pills. The
original report follows.

One correction to it: there are eight sliders, not seven, and **none of them
is brightness** — the Display page has no brightness control at all. That is a
missing feature rather than a broken one; see
`C-SETTINGS-DISPLAY-HAS-NO-BRIGHTNESS-CONTROL`.

---

**In short:** every slider in the Settings app — volume, brightness, night-light
warmth, text size, narrator voice rate, the two update-deferral sliders — draws
its current value correctly and cannot be moved with the mouse. The keyboard
arrow keys change some of them; the pointer changes none.

**Where:** `apps/settings/src/main.rs`, `PageSink::slider_row`. It registers no
click band at all (`self.row(label, None, …)`), with a comment saying so. The
drawing side, `render_slider`, is complete.

**How to confirm:** open Sound, drag the volume slider, watch it not move.

**The proper fix:** sliders need press-drag-release, not a click. That means the
page walk has to yield a band tagged with the slider's id *and* its track
geometry, plus a `dragging: Option<SliderId>` in `SettingsState` that
`MouseEventKind::Move` consults while a button is held. The value is then
`(mx - track_x) / SLIDER_WIDTH` clamped to 0..1, mapped back through whatever
range that slider covers — which means a `SliderId` enum carrying the range, in
the same shape as `ToggleId`/`toggle_mut`, so the mapping is written once. The
band and the track must come from one place, exactly as `pill_rect` now does for
pills; a slider whose visible track and draggable range differ is the same class
of defect this file was restructured to eliminate.
