## TD-FOUR-APPEARANCE-SETTINGS-THE-SHELL-STILL-IGNORES

**What.** `AppearanceSettings` has fifteen fields. The shell now honours eleven
of them. Four are read by nobody, and the settings panel offers each as a live
control, so choosing one changes nothing a user can see:

- `icon_size` (Small/Medium/Large/ExtraLarge) — the shell draws no icons at
  all. Taskbar buttons show truncated titles, start-menu rows show plain text,
  and `ManagedWindow::icon_id` indexes a registry nothing renders from.
  Blocked on the icon registry actually resolving to bitmaps
  (`gui/desktop/src/icons.rs` models the registry but is `#[allow(dead_code)]`).
- `cursor_size`, `cursor_scheme` — the pointer is drawn by the compositor, not
  by the shell. Correct owner, wrong wiring: nothing carries the value across.
  Needs the same treatment as the appearance channel in design-decisions.md
  §400 — the compositor has to be told, not asked.
- `animation_speed` — nothing in `main.rs` animates. Window open/close/minimize
  are instant state changes. `gui/desktop/src/animations.rs` has the curves and
  durations and is likewise `#[allow(dead_code)]`; hooking it up means the shell
  needs a frame clock, which it will get with the compositor event loop.

**Why it bites.** A setting that visibly does nothing is worse than a missing
one: the user concludes the whole panel is decorative. This was already true of
scaling, corners and shadows until they were wired up; these four are what is
left.

**Proper fix.** Each is blocked on a different piece of missing plumbing, so
they should be done as those arrive rather than in one pass: `icon_size` with
the icon registry, the two cursor settings with the compositor channel,
`animation_speed` with the event loop's frame clock. None should be faked in
the meantime — a shell that pretended to animate by jumping is not closer to
animating.

**Where.** `gui/appearance/src/lib.rs` — `AppearanceSettings`;
`gui/desktop/src/main.rs` — `DesktopShell::render_*`;
`gui/desktop/src/icons.rs`, `gui/desktop/src/animations.rs`.
