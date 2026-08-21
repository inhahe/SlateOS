//! Where a window goes when it is snapped to a zone.
//!
//! Seven named layouts, each dividing a work area into two to six
//! non-overlapping rectangles. A shell offers them in a picker; a compositor
//! places the window; and the two have to compute the *same* rectangle for the
//! same zone, or the window lands somewhere other than where the user aimed.
//!
//! That is why this lives in the protocol crate rather than in either end. The
//! wire verb for tiling names a layout and a zone within it — it cannot name
//! pixels, because the shell does not know the display bounds and is not
//! allowed to place windows. So "zone 3 of the six-cell grid" *is* protocol
//! content, in exactly the way [`WindowInfo`](crate::WindowInfo)'s field layout
//! is: a disagreement between the two ends about what it means is a protocol
//! bug, not a rendering one.
//!
//! Nothing here draws. The shell's picker and drag overlay — the parts that
//! turn these rectangles into something on screen — stay in the shell, along
//! with the per-window history that remembers where a window was before it was
//! snapped.

/// Inset (gap) between adjacent zones in pixels.
///
/// Public because both ends assert against the resulting geometry, and a
/// private constant would force each of them to re-state `6.0` — which is
/// how the shell ended up with a second, gapless snap implementation in the
/// first place (see `design-decisions.md` §469).
pub const ZONE_GAP: f32 = 6.0;

// ============================================================================
// SnapZone
// ============================================================================

/// Unique identifier for a snap zone within a layout.
pub type ZoneId = u32;

/// A single rectangular zone that a window can snap into.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapZone {
    /// Unique id within the parent layout.
    pub id: ZoneId,
    /// Horizontal position (pixels from left).
    pub x: f32,
    /// Vertical position (pixels from top).
    pub y: f32,
    /// Width in pixels.
    pub width: f32,
    /// Height in pixels.
    pub height: f32,
    /// Human-readable label (e.g. "Left", "Top-Right").
    pub label: &'static str,
}

impl SnapZone {
    /// Returns `true` when the point `(px, py)` lies inside this zone.
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }

    /// Centre point of the zone.
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

// ============================================================================
// SnapLayout & Presets
// ============================================================================

/// A named arrangement of zones covering a screen.
#[derive(Clone, Debug)]
pub struct SnapLayout {
    /// Display name for the layout (shown in the picker).
    pub name: &'static str,
    /// The zones that compose this layout.
    pub zones: Vec<SnapZone>,
}

/// Predefined layout presets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SnapLayoutPreset {
    /// Two equal vertical halves (left 50% / right 50%).
    TwoEqualHalves,
    /// Three equal vertical columns (33% each).
    ThreeColumns,
    /// Left column 66%, right column 33%.
    TwoThirdsLeft,
    /// Left column 33%, right column 66%.
    TwoThirdsRight,
    /// Four equal quadrants (2x2 grid).
    FourQuadrants,
    /// Left half + right top/bottom (3 zones).
    ThreeLeftTwoRight,
    /// Six-cell grid (3 columns x 2 rows).
    SixGrid,
}

impl SnapLayoutPreset {
    /// Display name for the picker UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::TwoEqualHalves => "Two Halves",
            Self::ThreeColumns => "Three Columns",
            Self::TwoThirdsLeft => "2/3 + 1/3",
            Self::TwoThirdsRight => "1/3 + 2/3",
            Self::FourQuadrants => "Quadrants",
            Self::ThreeLeftTwoRight => "Left + 2 Right",
            Self::SixGrid => "6-Cell Grid",
        }
    }

    /// All available presets in the order they appear in the picker.
    pub fn all() -> &'static [Self] {
        &[
            Self::TwoEqualHalves,
            Self::ThreeColumns,
            Self::TwoThirdsLeft,
            Self::TwoThirdsRight,
            Self::FourQuadrants,
            Self::ThreeLeftTwoRight,
            Self::SixGrid,
        ]
    }

    /// Build the concrete [`SnapLayout`] filling `area`.
    ///
    /// `area` is the **work area**, not the screen: the region left over once
    /// the taskbar has taken its strip. This used to take a screen width and
    /// height, and every arm below anchored its zones at `(0, 0)` over the
    /// full height — so a window snapped to any zone extended underneath the
    /// taskbar, hiding its own bottom edge. Nothing caught it because nothing
    /// calls this module yet, and because the zone tests assert relationships
    /// between zones (they tile, they do not overlap) which hold just as well
    /// over the wrong rectangle.
    ///
    /// The arms compute in area-local coordinates and the origin is added once
    /// at the end, so a new layout cannot forget to apply it.
    ///
    /// # The gap is an affordance, not an invariant
    ///
    /// Every arm subtracts [`ZONE_GAP`] from the area before dividing it, which
    /// is only meaningful when the area is big enough to give it away. At a
    /// three-pixel-wide work area `(3.0 - 6.0) / 2.0` is a **negative**
    /// half-width, and the second zone starts at 4.5 — outside the very
    /// rectangle it is meant to tile. Degenerate sizes are not hypothetical
    /// here: a one-pixel-wide display is a legal
    /// argument, and the shell builds work areas down to zero width.
    ///
    /// So the gap is *attempted* and kept only if it leaves every zone at
    /// least `ZONE_GAP` in both dimensions; otherwise the layout is rebuilt
    /// edge-to-edge. The rule is applied to the built zones rather than to each
    /// preset's arithmetic so that a preset added later inherits it without
    /// knowing it exists.
    pub fn build(self, area: WorkArea) -> SnapLayout {
        let spaced = self.zones_with_gap(area, ZONE_GAP);
        let zones = if spaced
            .iter()
            .all(|z| z.width >= ZONE_GAP && z.height >= ZONE_GAP)
        {
            spaced
        } else {
            self.zones_with_gap(area, 0.0)
        };

        // The one place the work-area origin is applied. Doing it per-arm
        // would be eleven chances to forget.
        let zones = zones
            .into_iter()
            .map(|z| SnapZone {
                x: z.x + area.x,
                y: z.y + area.y,
                ..z
            })
            .collect();

        SnapLayout {
            name: self.label(),
            zones,
        }
    }

    /// The preset's zones in **area-local** coordinates, separated by `g`.
    ///
    /// Split out of [`build`](Self::build) so the gap can be chosen by the
    /// caller and the whole layout re-derived without it. Coordinates are
    /// relative to the area's origin; `build` adds it.
    fn zones_with_gap(self, area: WorkArea, g: f32) -> Vec<SnapZone> {
        let screen_w = area.width;
        let screen_h = area.height;

        match self {
            Self::TwoEqualHalves => {
                let half_w = (screen_w - g) / 2.0;
                vec![
                    SnapZone {
                        id: 0,
                        x: 0.0,
                        y: 0.0,
                        width: half_w,
                        height: screen_h,
                        label: "Left",
                    },
                    SnapZone {
                        id: 1,
                        x: half_w + g,
                        y: 0.0,
                        width: half_w,
                        height: screen_h,
                        label: "Right",
                    },
                ]
            }
            Self::ThreeColumns => {
                let col_w = (screen_w - 2.0 * g) / 3.0;
                vec![
                    SnapZone {
                        id: 0,
                        x: 0.0,
                        y: 0.0,
                        width: col_w,
                        height: screen_h,
                        label: "Left",
                    },
                    SnapZone {
                        id: 1,
                        x: col_w + g,
                        y: 0.0,
                        width: col_w,
                        height: screen_h,
                        label: "Center",
                    },
                    SnapZone {
                        id: 2,
                        x: 2.0 * (col_w + g),
                        y: 0.0,
                        width: col_w,
                        height: screen_h,
                        label: "Right",
                    },
                ]
            }
            Self::TwoThirdsLeft => {
                let left_w = (screen_w - g) * 2.0 / 3.0;
                let right_w = screen_w - g - left_w;
                vec![
                    SnapZone {
                        id: 0,
                        x: 0.0,
                        y: 0.0,
                        width: left_w,
                        height: screen_h,
                        label: "Left 2/3",
                    },
                    SnapZone {
                        id: 1,
                        x: left_w + g,
                        y: 0.0,
                        width: right_w,
                        height: screen_h,
                        label: "Right 1/3",
                    },
                ]
            }
            Self::TwoThirdsRight => {
                let left_w = (screen_w - g) / 3.0;
                let right_w = screen_w - g - left_w;
                vec![
                    SnapZone {
                        id: 0,
                        x: 0.0,
                        y: 0.0,
                        width: left_w,
                        height: screen_h,
                        label: "Left 1/3",
                    },
                    SnapZone {
                        id: 1,
                        x: left_w + g,
                        y: 0.0,
                        width: right_w,
                        height: screen_h,
                        label: "Right 2/3",
                    },
                ]
            }
            Self::FourQuadrants => {
                let half_w = (screen_w - g) / 2.0;
                let half_h = (screen_h - g) / 2.0;
                vec![
                    SnapZone {
                        id: 0,
                        x: 0.0,
                        y: 0.0,
                        width: half_w,
                        height: half_h,
                        label: "Top-Left",
                    },
                    SnapZone {
                        id: 1,
                        x: half_w + g,
                        y: 0.0,
                        width: half_w,
                        height: half_h,
                        label: "Top-Right",
                    },
                    SnapZone {
                        id: 2,
                        x: 0.0,
                        y: half_h + g,
                        width: half_w,
                        height: half_h,
                        label: "Bottom-Left",
                    },
                    SnapZone {
                        id: 3,
                        x: half_w + g,
                        y: half_h + g,
                        width: half_w,
                        height: half_h,
                        label: "Bottom-Right",
                    },
                ]
            }
            Self::ThreeLeftTwoRight => {
                let left_w = (screen_w - g) / 2.0;
                let right_w = screen_w - g - left_w;
                let half_h = (screen_h - g) / 2.0;
                vec![
                    SnapZone {
                        id: 0,
                        x: 0.0,
                        y: 0.0,
                        width: left_w,
                        height: screen_h,
                        label: "Left",
                    },
                    SnapZone {
                        id: 1,
                        x: left_w + g,
                        y: 0.0,
                        width: right_w,
                        height: half_h,
                        label: "Top-Right",
                    },
                    SnapZone {
                        id: 2,
                        x: left_w + g,
                        y: half_h + g,
                        width: right_w,
                        height: half_h,
                        label: "Bottom-Right",
                    },
                ]
            }
            Self::SixGrid => {
                let col_w = (screen_w - 2.0 * g) / 3.0;
                let row_h = (screen_h - g) / 2.0;
                // One row of labels per `chunks(3)` row, so the label array's
                // own shape carries the grid. The previous form counted rows
                // and columns and then looked the label back up by index —
                // which needed an `unwrap_or(&"Zone")` fallback for an
                // out-of-range index that the loop bounds already made
                // impossible, i.e. a branch that could never run and could
                // never be tested. Iterating the labels removes the lookup,
                // and with it the dead branch.
                let labels = [
                    ["Top-Left", "Top-Center", "Top-Right"],
                    ["Bottom-Left", "Bottom-Center", "Bottom-Right"],
                ];
                labels
                    .iter()
                    .enumerate()
                    .flat_map(|(row, row_labels)| {
                        row_labels
                            .iter()
                            .enumerate()
                            .map(move |(col, label)| (row, col, *label))
                    })
                    .enumerate()
                    .map(|(idx, (row, col, label))| SnapZone {
                        id: idx as ZoneId,
                        x: col as f32 * (col_w + g),
                        y: row as f32 * (row_h + g),
                        width: col_w,
                        height: row_h,
                        label,
                    })
                    .collect()
            }
        }
    }
}

/// The rectangle snap zones tile: the screen minus the taskbar's strip.
///
/// A distinct type rather than four loose `f32`s because the previous
/// signature — `build(screen_w, screen_h)` — was *callable* with the work
/// area's size and still wrong, since it had nowhere to put the origin. A
/// caller that has only a screen must now say so explicitly, via
/// [`WorkArea::whole_screen`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkArea {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl WorkArea {
    /// A work area at an explicit origin.
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The whole screen, for a display with no reserved chrome.
    ///
    /// Named rather than implied, so that passing a screen size where a work
    /// area belongs is a decision someone wrote down.
    #[must_use]
    pub const fn whole_screen(width: f32, height: f32) -> Self {
        Self::new(0.0, 0.0, width, height)
    }

    /// The x coordinate one past the right edge.
    #[must_use]
    pub fn right(self) -> f32 {
        self.x + self.width
    }

    /// The y coordinate one past the bottom edge.
    #[must_use]
    pub fn bottom(self) -> f32 {
        self.y + self.height
    }
}

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    /// A 1080p screen with nothing subtracted.
    ///
    /// Most tests here are about proportions — that two halves are equal, that
    /// six zones do not overlap — and those hold at any origin, so they use
    /// this. The tests that are specifically about the *origin* use [`DESK`] and
    /// [`TOP_BAR_DESK`] below, because a work area that starts at (0, 0) and
    /// fills the screen is exactly the case in which the defect this file was
    /// fixed for is invisible.
    const SCREEN: WorkArea = WorkArea::whole_screen(1920.0, 1080.0);

    /// The same screen with a 48 px taskbar along the bottom: origin still at
    /// (0, 0), but 48 px shorter.
    const DESK: WorkArea = WorkArea::new(0.0, 0.0, 1920.0, 1032.0);

    /// The same screen with the taskbar along the *top* — an arrangement in
    /// which the work area's origin is not (0, 0), and therefore one that can
    /// catch a zone builder that ignores it.
    const TOP_BAR_DESK: WorkArea = WorkArea::new(0.0, 48.0, 1920.0, 1032.0);

    /// The same screen with a 64 px dock down the *left* side and a 64 px
    /// sidebar down the right.
    ///
    /// Present because `TOP_BAR_DESK` alone is not enough: it offsets only the
    /// `y` origin, so a test written against it is blind to code that drops the
    /// `x` one. Inset on *both* sides deliberately — with only the left inset
    /// its `right()` came to 1920, the screen width, so a right bound measured
    /// against the screen was still indistinguishable from a correct one.
    const SIDE_BAR_DESK: WorkArea = WorkArea::new(64.0, 0.0, 1792.0, 1080.0);

    // ======================================================================
    // SnapZone
    // ======================================================================

    #[test]
    fn zone_contains_interior_point() {
        let z = SnapZone {
            id: 0,
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
            label: "Test",
        };
        assert!(z.contains(50.0, 40.0));
    }

    #[test]
    fn zone_contains_top_left_corner() {
        let z = SnapZone {
            id: 0,
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
            label: "Test",
        };
        assert!(z.contains(10.0, 20.0));
    }

    #[test]
    fn zone_excludes_point_outside() {
        let z = SnapZone {
            id: 0,
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
            label: "Test",
        };
        assert!(!z.contains(5.0, 40.0));
        assert!(!z.contains(50.0, 80.0));
        assert!(!z.contains(111.0, 40.0));
    }

    #[test]
    fn zone_excludes_bottom_right_boundary() {
        // The zone uses exclusive right/bottom (< not <=).
        let z = SnapZone {
            id: 0,
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            label: "Test",
        };
        assert!(!z.contains(100.0, 25.0));
        assert!(!z.contains(50.0, 50.0));
    }

    #[test]
    fn zone_center_calculation() {
        let z = SnapZone {
            id: 0,
            x: 100.0,
            y: 200.0,
            width: 400.0,
            height: 300.0,
            label: "",
        };
        let (cx, cy) = z.center();
        assert!((cx - 300.0).abs() < f32::EPSILON);
        assert!((cy - 350.0).abs() < f32::EPSILON);
    }

    // ======================================================================
    // SnapLayoutPreset -- zone counts
    // ======================================================================

    #[test]
    fn preset_two_halves_produces_two_zones() {
        let layout = SnapLayoutPreset::TwoEqualHalves.build(SCREEN);
        assert_eq!(layout.zones.len(), 2);
        assert_eq!(layout.name, "Two Halves");
    }

    #[test]
    fn preset_three_columns_produces_three_zones() {
        let layout = SnapLayoutPreset::ThreeColumns.build(SCREEN);
        assert_eq!(layout.zones.len(), 3);
    }

    #[test]
    fn preset_two_thirds_left_produces_two_zones() {
        let layout = SnapLayoutPreset::TwoThirdsLeft.build(SCREEN);
        assert_eq!(layout.zones.len(), 2);
    }

    #[test]
    fn preset_two_thirds_right_produces_two_zones() {
        let layout = SnapLayoutPreset::TwoThirdsRight.build(SCREEN);
        assert_eq!(layout.zones.len(), 2);
    }

    #[test]
    fn preset_four_quadrants_produces_four_zones() {
        let layout = SnapLayoutPreset::FourQuadrants.build(SCREEN);
        assert_eq!(layout.zones.len(), 4);
    }

    #[test]
    fn preset_three_left_two_right_produces_three_zones() {
        let layout = SnapLayoutPreset::ThreeLeftTwoRight.build(SCREEN);
        assert_eq!(layout.zones.len(), 3);
    }

    #[test]
    fn preset_six_grid_produces_six_zones() {
        let layout = SnapLayoutPreset::SixGrid.build(SCREEN);
        assert_eq!(layout.zones.len(), 6);
    }

    #[test]
    fn all_presets_returns_seven() {
        assert_eq!(SnapLayoutPreset::all().len(), 7);
    }

    // ======================================================================
    // Layout geometry correctness
    // ======================================================================

    #[test]
    fn two_halves_covers_full_width() {
        let layout = SnapLayoutPreset::TwoEqualHalves.build(SCREEN);
        let left = &layout.zones[0];
        let right = &layout.zones[1];
        let total = left.width + ZONE_GAP + right.width;
        assert!((total - 1920.0).abs() < 0.1);
    }

    #[test]
    fn two_halves_zones_are_equal_width() {
        let layout = SnapLayoutPreset::TwoEqualHalves.build(SCREEN);
        let diff = (layout.zones[0].width - layout.zones[1].width).abs();
        assert!(diff < 0.1);
    }

    #[test]
    fn three_columns_cover_full_width() {
        let layout = SnapLayoutPreset::ThreeColumns.build(SCREEN);
        let total: f32 = layout.zones.iter().map(|z| z.width).sum::<f32>() + 2.0 * ZONE_GAP;
        assert!((total - 1920.0).abs() < 0.5);
    }

    #[test]
    fn two_thirds_left_ratio_approximately_correct() {
        let layout = SnapLayoutPreset::TwoThirdsLeft.build(SCREEN);
        let left = &layout.zones[0];
        let right = &layout.zones[1];
        // left should be roughly twice the right.
        let ratio = left.width / right.width;
        assert!(ratio > 1.8 && ratio < 2.2, "ratio was {ratio}");
    }

    #[test]
    fn four_quadrants_cover_full_area() {
        let layout = SnapLayoutPreset::FourQuadrants.build(SCREEN);
        // Sum of zone areas + gap areas should approximately equal screen area.
        let zone_area: f32 = layout.zones.iter().map(|z| z.width * z.height).sum();
        let screen_area = 1920.0 * 1080.0;
        // Allow for gap space.
        assert!(zone_area > screen_area * 0.98);
    }

    #[test]
    fn six_grid_zones_have_unique_ids() {
        let layout = SnapLayoutPreset::SixGrid.build(SCREEN);
        let mut ids: Vec<ZoneId> = layout.zones.iter().map(|z| z.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 6);
    }

    #[test]
    fn six_grid_ids_run_row_major_and_match_their_labels() {
        // The id is what `edge_to_default_snap` and the caller's persisted
        // window state refer to, and it is now the label array's index rather
        // than a separately-computed `row * 3 + col`. Pin the correspondence:
        // renumbering these silently sends every remembered window to a
        // different corner.
        let layout = SnapLayoutPreset::SixGrid.build(SCREEN);
        let named: Vec<(ZoneId, &str)> = layout.zones.iter().map(|z| (z.id, z.label)).collect();
        assert_eq!(
            named,
            vec![
                (0, "Top-Left"),
                (1, "Top-Center"),
                (2, "Top-Right"),
                (3, "Bottom-Left"),
                (4, "Bottom-Center"),
                (5, "Bottom-Right"),
            ]
        );
        // And the labels describe where the zones actually are.
        let mid_x = SCREEN.x + SCREEN.width / 2.0;
        let mid_y = SCREEN.y + SCREEN.height / 2.0;
        for z in &layout.zones {
            let (cx, cy) = z.center();
            assert_eq!(
                cy < mid_y,
                z.label.starts_with("Top"),
                "{} is labelled {:?} but its centre y is {cy}",
                z.id,
                z.label
            );
            if z.label.ends_with("Left") {
                assert!(cx < mid_x, "{:?} is not on the left", z.label);
            } else if z.label.ends_with("Right") {
                assert!(cx > mid_x, "{:?} is not on the right", z.label);
            }
        }
    }

    #[test]
    fn six_grid_zones_do_not_overlap() {
        let layout = SnapLayoutPreset::SixGrid.build(SCREEN);
        for (i, a) in layout.zones.iter().enumerate() {
            for b in layout.zones.iter().skip(i + 1) {
                let overlap_x = a.x < b.x + b.width && a.x + a.width > b.x;
                let overlap_y = a.y < b.y + b.height && a.y + a.height > b.y;
                assert!(
                    !(overlap_x && overlap_y),
                    "zones {} and {} overlap",
                    a.id,
                    b.id
                );
            }
        }
    }

    // ======================================================================
    // Zones stay inside the work area
    // ======================================================================

    #[test]
    fn every_zone_of_every_preset_stays_inside_the_work_area() {
        // The defect this file was fixed for. `build` anchored every zone at
        // (0, 0) with the *full screen* height, though the module doc has
        // always said the zones cover the work area — so every snapped window
        // ran underneath the taskbar and hid its own bottom edge. Nothing in
        // the suite could see it: the zone tests all assert *relationships*
        // between zones (they tile, they do not overlap, the halves are
        // equal), and those hold just as well over the wrong rectangle.
        //
        // Checked against work areas whose origin is *not* (0, 0), on both
        // axes separately: against an area that begins at the origin, a
        // builder that ignores the origin is indistinguishable from one that
        // honours it, and against a top-bar area alone, one that drops only
        // the `x` offset is equally invisible.
        for area in [TOP_BAR_DESK, SIDE_BAR_DESK, DESK] {
            for &preset in SnapLayoutPreset::all() {
                let layout = preset.build(area);
                assert!(
                    !layout.zones.is_empty(),
                    "{preset:?} produced no zones at all"
                );
                for z in &layout.zones {
                    assert!(
                        z.x >= area.x - 0.01,
                        "{preset:?} zone {} starts left of work area {area:?} ({} < {})",
                        z.id,
                        z.x,
                        area.x
                    );
                    assert!(
                        z.y >= area.y - 0.01,
                        "{preset:?} zone {} starts above work area {area:?} ({} < {}) \
                         — it would sit behind a top taskbar",
                        z.id,
                        z.y,
                        area.y
                    );
                    assert!(
                        z.x + z.width <= area.right() + 0.01,
                        "{preset:?} zone {} runs past the right edge of {area:?} ({} > {})",
                        z.id,
                        z.x + z.width,
                        area.right()
                    );
                    assert!(
                        z.y + z.height <= area.bottom() + 0.01,
                        "{preset:?} zone {} runs past the bottom edge of {area:?} ({} > {}) \
                         — a window snapped here would hide its own bottom edge \
                         behind the taskbar",
                        z.id,
                        z.y + z.height,
                        area.bottom()
                    );
                }
            }
        }
    }

    #[test]
    fn a_work_area_too_small_for_the_gap_gives_it_up_rather_than_going_negative() {
        // Found by wiring this module into the shell, which snaps on work areas
        // the shell's own tests take down to zero width. Every arm of
        // `zones_with_gap` subtracts the gap before dividing, so at three
        // pixels wide `(3 - 6) / 2` is a *negative* half-width and the second
        // zone starts at 4.5 — outside the rectangle it is tiling. The
        // in-bounds test above could not see it because it only ever ran on
        // desktop-sized areas, where the subtraction is free.
        for w in [
            0.0_f32, 1.0, 2.0, 3.0, 7.0, 11.0, 12.0, 17.0, 18.0, 19.0, 40.0,
        ] {
            for h in [0.0_f32, 1.0, 5.0, 6.0, 13.0, 40.0] {
                let area = WorkArea::new(3.0, 4.0, w, h);
                for &preset in SnapLayoutPreset::all() {
                    for z in &preset.build(area).zones {
                        assert!(
                            z.width >= 0.0 && z.height >= 0.0,
                            "{preset:?} zone {} of {area:?} has negative extent {}x{}",
                            z.id,
                            z.width,
                            z.height
                        );
                        assert!(
                            z.x >= area.x - 0.01 && z.y >= area.y - 0.01,
                            "{preset:?} zone {} of {area:?} starts outside it at ({}, {})",
                            z.id,
                            z.x,
                            z.y
                        );
                        assert!(
                            z.x + z.width <= area.right() + 0.01
                                && z.y + z.height <= area.bottom() + 0.01,
                            "{preset:?} zone {} of {area:?} ends outside it at ({}, {})",
                            z.id,
                            z.x + z.width,
                            z.y + z.height
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_work_area_with_room_for_the_gap_still_gets_one() {
        // The other half of the rule above: giving the gap up whenever it is
        // inconvenient would satisfy that test completely, and would silently
        // undo the separation between zones on every real screen.
        let zones = SnapLayoutPreset::TwoEqualHalves.build(DESK).zones;
        let seam = zones[0].x + zones[0].width;
        assert!(
            (zones[1].x - seam - ZONE_GAP).abs() < 0.01,
            "expected a {ZONE_GAP}px gap, zone 0 ends at {seam} and zone 1 starts at {}",
            zones[1].x
        );
    }

    #[test]
    fn a_work_area_shorter_than_the_screen_produces_shorter_zones() {
        // The bottom-taskbar case: the origin is unchanged, so only the
        // *height* can reveal whether the taskbar was subtracted.
        let full = SnapLayoutPreset::TwoEqualHalves.build(SCREEN);
        let desk = SnapLayoutPreset::TwoEqualHalves.build(DESK);
        assert!(
            (full.zones[0].height - desk.zones[0].height - 48.0).abs() < 0.01,
            "full-screen zone is {} tall, work-area zone is {} — expected a 48 px difference",
            full.zones[0].height,
            desk.zones[0].height
        );
    }
}
