## C-THE-SNAP-SUBSYSTEM-TILED-THE-SCREEN-INSTEAD-OF-THE-WORK-AREA (lane C, 2026-08-18) — FIXED

**Where:** `gui/desktop/src/snap.rs`

Three defects in the window-snap subsystem, all of which would have surfaced
only on the day someone wired it up, and none of which the module's own 60-odd
tests could have caught.

**What the code did.**

1. `SnapLayoutPreset::build(screen_w, screen_h)` anchored every zone at
   `(0, 0)` and gave it the full screen height, though the module doc has
   always said the zones cover "the work area". Every snapped window would
   have extended underneath the taskbar and hidden its own bottom edge.
2. `detect_edge` measured against the full screen, so the bottom edge and both
   bottom corners lay *inside* the taskbar's strip — simultaneously unreachable
   as snaps and stealing input from the bar.
3. `edge_to_default_zone` sent the *vertical* edges to the *horizontal* halves:
   `Top => (TwoEqualHalves, 0)` (commented `// maximize hint`, which it was
   not) and `Bottom => (TwoEqualHalves, 1)`. Dragging a window to the top moved
   it left; dragging it to the bottom moved it right.

Found while fixing these, in the same file:

4. `is_in_picker_trigger` was `cursor_y < top + THRESHOLD` with no lower bound,
   so with the taskbar at the top of the screen the whole taskbar was inside
   the picker's trigger band. `detect_edge` had the same shape on all four
   sides.
5. `update_picker_hover` and `render_picker` each computed the thumbnail grid
   from their own copy of the same expression, 400 lines apart — an arrangement
   in which editing one padding constant gives a picker that highlights one
   thumbnail and selects another.
6. The `SixGrid` arm looked its labels up by a computed index and carried an
   `unwrap_or(&"Zone")` fallback for an out-of-range index the loop bounds
   already made impossible: a branch that could never run and could never be
   tested.

**Why it survived review.** The suite was green and *could not have failed* for
any of these. `six_grid_zones_do_not_overlap`, `two_halves_covers_full_width`
and `four_quadrants_cover_full_area` all assert *relationships between zones* —
and a layout translated bodily off the work area tiles just as perfectly as one
on it. `all_edges_map_to_valid_zones` asserted only that each edge mapped to a
zone that **exists**, which `Top => left half` satisfies completely. This is the
same shape as `C-THE-CREDENTIAL-MANAGER-CHECKED-ITS-MASTER-PASSWORD-WITH-DJB2`
and the lock screen's FIPS-vector tests over a store with no salt in it: a green
suite asserting the properties the implementation happens to have rather than
the ones its caller depends on.

**The fix.** See `design-decisions.md` §467. `build` now takes a `WorkArea`
`{x, y, width, height}` and applies the origin at one `.map()` after the match,
so no arm can forget it; `detect_edge` and `is_in_picker_trigger` are bounded
on both sides; `edge_to_default_snap` returns `Option<EdgeSnap>` with
`Top => Maximize` and `Bottom => None`; the picker grid lives in one
`thumb_origin` helper used by both the hit-test and the draw;
`picker_items_per_row` returns `NonZeroUsize` so the two divisions are
infallible by type; the `SixGrid` labels are a `[[&str; 3]; 2]` iterated
directly, which deletes the dead fallback. Also cleared all seven of the file's
pre-existing `arithmetic_side_effects` warnings.

**Tests.** Eleven new tests, each verified to discriminate by reintroducing the
exact defect it is meant to catch and confirming it, and only it, failed. Two
fixture-design points came out of that exercise and are worth remembering:
a fixture must offset `x` and `y` **separately** (with only a top-taskbar
fixture, "`build` drops the `x` origin" failed *nothing*), and a fixture's
`right()`/`bottom()` must not coincidentally equal the screen's (the first
left-dock fixture was `x=64, width=1856`, so `right() == 1920` and a
screen-relative right bound was still indistinguishable from a correct one).
