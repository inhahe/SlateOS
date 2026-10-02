## TD-C-A-POINTER-SPEED-CHANGE-DOES-NOT-REACH-THE-POINTER (lane C, 2026-08-24) — FIXED 2026-08-24

**In short:** the Settings → Mouse page can change the pointer speed, the
acceleration profile, the button mapping and the key-repeat rate, and the file
it writes is read by the compositor — but only one setting out of that file is
adopted while the desktop is running. The rest take effect at the next login. A
user who drags the speed slider sees nothing happen.

**What.** `Compositor::reload_input` (`gui/compositor/src/lib.rs`) is what a
`ReloadInput` request lands in. It does:

```rust
let settings = inputsettings::InputFile::load().settings;
self.set_double_click_ms(settings.mouse.double_click_ms);
```

Everything else in `InputSettings` — `mouse.speed`, `accel_profile`,
`accel_gain`, `accel_threshold`, `natural_scroll`, `scroll_speed`,
`button_mapping`, and the whole of `keyboard` — is used by
`present::evdev::EvdevInput`, which has a `set_settings` for exactly this
purpose and no caller. `reload_input` cannot reach it: the `EvdevInput` lives
inside a `Paired` inside the `Present` that `Server::run_with` holds, and
`Compositor` has no reference to its own display.

**Why it was not fixed in the same change.** It is not a missing line; it is a
missing direction. `Compositor` is deliberately display-agnostic — that is what
lets the same compositor run headless, into a recording, onto a Win32 window and
onto a DRM card — so "the compositor tells the display to reload" needs a route
that does not put a display type into `Compositor`. The route that fits the
existing shapes is for `Server::run_with` to notice the request instead: it
already owns both the compositor and the `&mut dyn Present`, and it already
drives the loop that would apply it.

**The proper fix.** Add a `Present::reload_input(&mut self, settings:
&InputSettings)` with an empty default body — the same pattern
`Present::monitors` already uses for a capability only one implementor has —
have `Paired` forward it to `InputSource`, add the matching
`InputSource::reload_input` forwarding to `EvdevInput::set_settings`, and have
`Server::run_with` call it when the tick reports that a `ReloadInput` was
handled. `Compositor::reload_input` then returns the loaded `InputSettings`
rather than swallowing them, so the file is read once rather than twice.

**How to reproduce.** Not reproducible on the dev machine today, because the
capability grant is not landed and there is no `EvdevInput` to reload. On
hardware: set Settings → Mouse → pointer speed to its maximum, apply, and move
the mouse. The double-click interval will have changed (that one setting works);
the pointer speed will not, until the desktop is restarted.

**Severity.** Medium. The setting is not lost — it is in `input.yaml` and is
honoured at the next start — so this is a latency bug, not a data bug. But a
slider that appears to do nothing is indistinguishable to a user from a slider
that is broken, and it is the *second* time this exact shape has been found in
this area (`TD-C-THE-MOUSE-SETTINGS-PANEL-REACHES-NOTHING` was the first).

### 2026-08-24 — fixed, polled rather than told, and one thing the plan above got wrong

The chain now runs end to end: `Compositor::set_input_settings` keeps the whole
`InputSettings` (applying the one it is itself the consumer of),
`Server::reconcile_input` polls `Compositor::input_settings` once a tick and
pushes any *change* into `Present::reload_input`, `Paired` forwards that to its
input half, and `EvdevInput::reload_input` calls the `set_settings` that had
been sitting there with no caller. `main` no longer reads `input.yaml` a second
time; it takes the compositor's copy, so the file is read once.

**The plan above said "call it when the tick reports that a `ReloadInput` was
handled". That is not what landed, and deliberately.** A flag or a queue saying
"a reload happened" is a second description of the settings, and a second
description is a thing that can disagree with the first — a dropped
notification is a preference that silently never arrives, and a display
attached mid-session starts stale unless someone remembers to re-arm the flag.
`present.rs` already documents the alternative for `Present::monitors`: polled,
idempotent, `None` meaning "no opinion". Using a push for input settings and a
poll for monitor hotplug — two problems of identical shape, three lines apart
in the same loop body — would have been two mechanisms where one does. The
reasoning, including the cost (a `PartialEq` on a small struct per frame,
forever) and why `None` must not be spelled `InputSettings::default()`, is
`design-decisions.md` §548.

**Proved by `scripts/reintro-input-settings.py`** — ten one-line defects, one
per link in the chain, all caught, all restored by SHA-256.
