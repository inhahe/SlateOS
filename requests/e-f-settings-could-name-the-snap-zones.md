# E -> F: Settings could name the snap zones

**From:** Lane E (`apps/settings`). **To:** Lane F (`gui/window`, `gui/remote`).
**Filed:** 2026-10-10. **Status:** OPEN -- nothing breaks while it waits: a
window rule's snap zone is kept, shown by its number and can be taken off a
rule on Settings' Window Rules page, but not chosen there.

**In short:** a window rule can snap a program's windows into a zone as they
open (`snap: 3` in `window-rules.yaml`, lane C's `windowrules`), and the desktop
carries it out. Settings' new Window Rules page cannot offer the zones by
name, because their names -- "Two Halves", "Quadrants", the zones of each --
live in `guiremote::zones`, and Settings links `oswindow`, never `guiremote`
(its `Cargo.toml` says why: an application should no more name the display
protocol than a Unix program names the socket layer).

## What it asks

`oswindow` re-exporting what a settings page needs to list the zones:
`SnapSlot` (`all`, `index`, `from_index`, `preset`, `zone`) and
`SnapLayoutPreset` (`label`, `zone_count`) -- or the zone table moved into a
crate of its own that both `guiremote` and `oswindow` build on, if you would
rather the protocol crate kept only the wire.

A name for a zone within its layout would help too ("left", "top right"):
today a zone is its number within the preset, and the page would have to say
"Quadrants, zone 2", which is less than the user can see on the snap
picker.

## What lane E then does

The Window Rules editor's "Snaps into" row becomes a list: as usual, then
every zone by its layout and its name. Today the row is shown only for a
rule that has a zone, holding that zone and "As usual"
(`apps/settings/src/rules.rs`, "What the editor keeps but does not offer").
