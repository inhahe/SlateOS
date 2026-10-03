## §467 — Snap zones tile the work area, not the screen, and the edge a window is dragged to determines where it goes

**Date:** 2026-08-18 · **Lane:** C · **Decided by:** Claude (autonomous)

**In short:** The desktop has a Windows-11-style "snap" feature — drag a window
to the edge of the screen and it fills half the screen, or a quarter, or one
cell of a grid. Two things about it were wrong. First, it laid its zones out
over the *whole screen* including the strip the taskbar occupies, so a snapped
window would have run underneath the taskbar and hidden its own bottom edge.
Second, dragging a window to the **top** of the screen moved it to the **left
half**, and dragging it to the **bottom** moved it to the **right half** — the
direction you dragged had nothing to do with where the window went. Neither
could be seen by anyone, because nothing calls this code yet. It is now laid
out over the *work area* (the screen minus the taskbar), and the top edge
maximises while the bottom edge does nothing.

### The two decisions

**1. `build` takes a work area, not a screen size.**

`SnapLayoutPreset::build(screen_w, screen_h)` became
`build(area: WorkArea)`, where `WorkArea` is `{ x, y, width, height }`. The
eleven layout arms still compute in area-local coordinates — they are textually
unchanged — and the origin is added once, at a single `.map()` after the match:

```rust
let zones = zones.into_iter()
    .map(|z| SnapZone { x: z.x + area.x, y: z.y + area.y, ..z })
    .collect();
```

*Alternative considered:* thread `area.x` / `area.y` through each arm. Rejected:
eleven arms is eleven chances to forget, and the forgetting is invisible — a
zone at the wrong origin still tiles, still fails to overlap its neighbours,
and still passes every test the module had.

`WorkArea::whole_screen(w, h)` exists as a **named** constructor rather than
letting callers write `WorkArea::new(0.0, 0.0, w, h)`, so that substituting a
screen for a work area is a thing someone wrote down rather than a thing that
happened.

**2. The edge-to-snap mapping is `Option<EdgeSnap>`, and the vertical edges no
longer map to horizontal halves.**

| Edge | Was | Is |
|---|---|---|
| Left / Right | left half / right half | unchanged |
| **Top** | **left half** (commented "maximize hint", which it was not) | **maximise** |
| **Bottom** | **right half** | **nothing — an ordinary window move** |
| Corners | four quadrants | unchanged |

Top maximising is what every desktop this imitates does. Bottom doing *nothing*
is the honest answer: there is no half-height bottom strip among the presets,
and inventing one to fill the table is a worse outcome than leaving the drag
alone. Expressing that required the return type to become `Option`, and
maximise-is-not-a-zone required a new `EdgeSnap::Maximize` variant beside
`EdgeSnap::Zone(preset, id)` — the manager synthesises a full-work-area zone
for it at hit-test time.

*Alternative considered:* keep returning a `(preset, zone_id)` pair and add a
`Maximized` pseudo-preset whose single zone is the whole area. Rejected: it
makes "maximise" a layout the picker would have to hide, and puts a
special-case in every consumer that enumerates presets.

### Why this is filed as a decision rather than a bug fix

Because of what the tests taught. All three defects lived under a **green
suite** that could not have failed for any of them:

- `six_grid_zones_do_not_overlap` and `two_halves_covers_full_width` assert
  *relationships between zones*. Those hold just as well over the wrong
  rectangle. A layout translated bodily off the work area tiles perfectly.
- `all_edges_map_to_valid_zones` asserted only that each edge mapped to a zone
  that **exists**. Top → left half satisfies it completely.

The replacement tests assert the property the *caller* depends on, not the one
the implementation happens to have: every zone of every preset lies inside the
work area; the zone an edge maps to has its centre on the side of the work area
the user dragged toward.

Two fixture-design points, both found by reintroducing defects rather than by
reasoning:

- The fixtures must offset **x and y separately**. With only a top-taskbar
  fixture, "`build` drops the `x` origin" failed nothing.
- A fixture's `right()` must not coincidentally equal the screen width. The
  original left-dock fixture was `x=64, width=1856` → `right() == 1920`, and a
  right edge measured against the screen was still indistinguishable from a
  correct one. It is now inset on both sides.

### What this does not settle

`gui/desktop/src/main.rs::snap_window` is a **second, wired** implementation of
this feature — integer geometry, no inter-zone gap, half/quarter only. `snap.rs`
is `f32` with a 6 px gap and seven presets, and has no caller at all. Reconciling
them is a follow-on task; see `known-issues.md`
`C-TWO-SNAP-IMPLEMENTATIONS-WITH-DIFFERENT-GAP-POLICIES`. This entry decides
only what `snap.rs` does, on the grounds that the moment agreement between two
implementations is cheap is *before* the wiring exists.
