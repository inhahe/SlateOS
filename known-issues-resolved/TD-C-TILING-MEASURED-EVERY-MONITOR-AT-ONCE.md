## TD-C-TILING-MEASURED-EVERY-MONITOR-AT-ONCE (lane C, 2026-08-21) — RESOLVED 2026-08-21

**What was wrong.** Every tiling operation in the compositor — maximize, the
`SnapEdge` half-screen snap, the `SnapSlot` zone snap, and the edge-drag drop
that had just been built on top of them — measured itself against the *union of
all connected displays* rather than against the monitor the window was on. The
one place it was decided was:

```rust
fn snap_area(&self) -> WorkArea {
    work_area_of(self.display_manager.virtual_bounds())
}
```

`virtual_bounds()` is the bounding box of every `Display`, which is the correct
answer for "how big is the desktop" and the wrong one for "how big may this
window get". On a two-monitor desktop of an 800x600 primary and a 1024x768
secondary to its right, maximizing a window sitting on the *second* monitor
moved it to `x = 0` and made it 1824 px wide — it filled both screens and its
title bar landed on the one the user was not using. Snapping it left gave it the
whole first monitor plus a 112 px strip of the second.

**Why nothing caught it.** A single-monitor compositor cannot tell the two
readings apart: with one display, `virtual_bounds()` *is* that display's bounds.
Every existing tiling test built a one-display `Compositor::new(800, 600, …)`,
so all of them agreed with the buggy code and would have agreed with any correct
one. `DisplayManager::display_for(&Rect)` — largest-intersection lookup with a
primary-display fallback — had existed the whole time and was used by the
per-monitor DPI scaling path; the tiling paths simply never called it. This is
the failure mode where the fixture, not the assertion, is what is missing.

**The fix** (commit below). `snap_area()` is gone. In its place:

| Method | Answers |
| `work_bounds_for(rect)` | the bounds of the display holding most of `rect`, falling back to `virtual_bounds()` only when *no* display is connected |
| `work_area_for(rect)` | the same as a `WorkArea` |
| `work_area_at(x, y)` | the work area of the monitor under a point |
| `work_area_of_window(id)` | the work area of the monitor under a window's frame |
| `work_rect(area)` (free fn) | the inverse of `work_area_of`, so a work area can go back to whole pixels |

`maximize_window` and `snap_window_to_zone` each split into a public form that
resolves the window's own monitor and a private `_within` form taking a
caller-chosen area. The edge drop uses the `_within` forms, because the drop
must follow the monitor **under the pointer** — see `design-decisions.md` §509 —
and `DropIntent` now carries the `WorkArea` the preview was drawn against so
that preview and drop cannot disagree.

**Coverage added** — eight tests, and every one of them fails against the old
code, checked by reintroducing each defect individually:
`maximizing_fills_the_windows_own_monitor_and_not_every_monitor`,
`snapping_takes_half_of_the_windows_own_monitor`,
`a_zone_snap_resolves_against_the_windows_own_monitor`,
`an_edge_drop_uses_the_monitor_the_pointer_is_over`,
`the_preview_crosses_the_seam_with_the_pointer`,
`the_interior_seam_is_two_edges_and_not_a_middle`,
`a_drop_tiles_the_monitor_the_pointer_is_over_even_when_the_window_is_not`,
`a_work_area_survives_the_round_trip_back_to_pixels`. The shared `two_monitors`
fixture is the thing that was missing; prefer it over `Compositor::new` for any
future geometry test, since a one-display compositor cannot distinguish "this
screen" from "all screens".

Two traps that cost time and are worth knowing before writing the ninth such
test: (1) a zone rectangle carries the layout's gap, so a hand-computed "half of
1024" (512) is not the number a drop produces (509) — derive expectations from
`guiremote::zones` itself, which is what the `edge_drop_rect` helper does; and
(2) two synchronous drags of the same title bar fall inside the default
double-click interval, so the second press is read as a maximize and starts no
drag — use a fresh compositor per case or `set_double_click_ms(2000)`.

**What remains.** The work area is still the *whole* of the correct monitor,
including the strip the taskbar occupies — that is
`TD-C-COMPOSITOR-TILES-UNDER-THE-TASKBAR`, still open, and `work_bounds_for` is
now the single place its fix has to land.

*(Update 2026-08-21: that is now done — `work_area_for` subtracts what panels
have reserved. See the entry above.)*
