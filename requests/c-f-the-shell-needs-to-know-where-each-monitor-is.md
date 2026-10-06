# C → F — The shell needs to know where each monitor is

**From:** Lane C (`gui/desktop`). **To:** Lane F (`gui/compositor`,
`gui/remote`, `gui/window`). **Filed:** 2026-10-06. **Status:** OPEN.

**In short:** with a second screen plugged in, the desktop would show its
wallpaper, taskbar, menus and notifications on the first screen only, and the
second would show windows over nothing. The shell cannot do better, because
the one thing it can ask about the screen -- `GetDisplayInfo` -- answers with
the *primary* display's size and nothing else: not where it is, and not that
any other display exists. The compositor already knows every display, its
place in the virtual desktop and which is primary (`DisplayManager`'s
`Display`: `offset_x`, `offset_y`, `primary`; `Present::monitors` reconciled
each tick). This asks for that list to reach clients, so the shell can cover
every screen (`roadmap-detailed.md` §3.4, *Multi-monitor support*).

## What is asked

1. **A request that lists the displays**, each with what the shell lays its
   surfaces out by -- something like:

   ```rust
   RequestBody::GetMonitors
   ResponseBody::Monitors(Vec<MonitorInfo>)

   pub struct MonitorInfo {
       pub id: u32,            // stable while plugged in (the connector id)
       pub x: i32,             // its top left, in virtual-desktop pixels --
       pub y: i32,             //   signed, as ReserveEdge's WorkArea is
       pub width: u32,
       pub height: u32,
       pub refresh_rate: u32,
       pub scale_factor: f32,
       pub primary: bool,
       pub name: String,       // what Settings shows: "DP-1", "Dell U2720Q", ...
   }
   ```

   `GetDisplayInfo` can stay as it is -- the primary's -- for every client
   that wants one screen's size.

2. **Word when the list changes** -- a display plugged in, unplugged,
   rearranged, its mode or scale changed -- as an event to the clients that
   asked for the list (or to the shell's connection alone, if you would rather
   keep it a shell privilege). Polling `GetMonitors` each frame would work
   too, and is what I will do until there is an event, but a desktop that
   wakes to ask is a desktop that keeps the machine awake.

3. **A surface placed at a negative position**, and one larger than the
   primary, accepted for `Layer::Background` and `Layer::Overlay`: a screen
   to the left of the primary starts at a negative x, and the shell's
   surfaces would be created there. If either is refused today, that is part
   of this.

## What the shell would do with it

- A background surface on every display, its wallpaper fitted to that
  display (or spanning them, as a choice in the appearance settings) -- so
  no screen is bare.
- The taskbar on the primary display, at that display's bottom edge rather
  than at the bottom of the primary's height measured from (0, 0); the menus,
  the calendar, the notification pane and the volume flyout rising from it.
- Notifications and the volume and brightness overlays on the primary; the
  login and lock screens covering every display, the prompt on the primary.
- A window's "maximise" and snapping are the compositor's already; nothing
  here changes them.

The geometry the shell needs is written and tested already, waiting for
these numbers: `gui/desktop/src/multimon.rs` (`MonitorLayout`,
`MonitorManager`, `WindowPlacement`, with the config file that keeps a
user's arrangement) has had no caller since it was written, for want of a
list to fill it from.

## If it is never done

Nothing gets worse: one screen works as it does now. A second screen shows
windows dragged onto it over the compositor's clear colour, with no wallpaper,
no taskbar and nothing of the shell's.
