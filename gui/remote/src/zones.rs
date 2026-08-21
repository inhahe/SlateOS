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

    /// How many zones this preset divides a work area into.
    ///
    /// Stated separately from [`build`](Self::build) rather than derived from
    /// it because [`SnapSlot`] needs the count without a work area to build
    /// against — a wire byte has to be checked for validity before anyone knows
    /// what display it will be resolved on. `zone_counts_match_the_zones_built`
    /// keeps the two honest.
    #[must_use]
    pub const fn zone_count(self) -> u8 {
        match self {
            Self::TwoEqualHalves | Self::TwoThirdsLeft | Self::TwoThirdsRight => 2,
            Self::ThreeColumns | Self::ThreeLeftTwoRight => 3,
            Self::FourQuadrants => 4,
            Self::SixGrid => 6,
        }
    }

    /// The wire index of this preset's zone 0.
    ///
    /// The presets are laid out end to end in the order of [`all`](Self::all),
    /// each taking [`zone_count`](Self::zone_count) indices, so every
    /// (preset, zone) pair has an index of its own. **These offsets are wire
    /// format**: changing a preset's zone count, or the order of `all`, moves
    /// every later preset's zones onto different bytes, and a peer built
    /// against the old numbering would tile windows into the wrong rectangles
    /// rather than failing. Add new presets at the end.
    const fn first_slot(self) -> u8 {
        match self {
            Self::TwoEqualHalves => 0,
            Self::ThreeColumns => 2,
            Self::TwoThirdsLeft => 5,
            Self::TwoThirdsRight => 7,
            Self::FourQuadrants => 9,
            Self::ThreeLeftTwoRight => 13,
            Self::SixGrid => 16,
        }
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

// ============================================================================
// SnapSlot -- one (layout, zone) pair, which is what travels
// ============================================================================

/// A place a window can be tiled to, named without naming pixels.
///
/// This is the payload of the protocol's zone-tiling verb, and it is a type
/// rather than a loose `(preset, zone)` pair for one reason: `zone` is only
/// meaningful against the preset it belongs to. `(TwoEqualHalves, 5)` is
/// nothing — the two-halves layout has zones 0 and 1 — but nothing about a pair
/// of integers says so, and a compositor handed one would have to decide at the
/// far end of the wire what to do with a request that never made sense. The
/// fields are private and [`new`](Self::new) is the only way in, so an invalid
/// slot cannot be built at all, and the encoding is therefore total: every
/// `SnapSlot` has a byte and every accepted byte has a `SnapSlot`.
///
/// There are [`COUNT`](Self::COUNT) of them across the seven presets, which is
/// what lets the tiling verb keep this protocol's one-byte-per-action rule
/// exactly rather than bending it: the slot is folded into the same byte as the
/// action, so no reader or writer of a frame grows a special case for a nested
/// payload. See [`ShellControlAction`](crate::control::ShellControlAction).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SnapSlot {
    preset: SnapLayoutPreset,
    zone: u8,
}

impl SnapSlot {
    /// How many slots exist across every preset.
    ///
    /// The sum of every preset's [`zone_count`](SnapLayoutPreset::zone_count);
    /// `count_is_the_sum_of_every_presets_zones` holds the two together.
    pub const COUNT: u8 = 22;

    /// The slot for `zone` of `preset`, or `None` if that preset has no such
    /// zone.
    #[must_use]
    pub const fn new(preset: SnapLayoutPreset, zone: u8) -> Option<Self> {
        if zone < preset.zone_count() {
            Some(Self { preset, zone })
        } else {
            None
        }
    }

    /// The layout this slot belongs to.
    #[must_use]
    pub const fn preset(self) -> SnapLayoutPreset {
        self.preset
    }

    /// Which zone of that layout, counting from zero.
    #[must_use]
    pub const fn zone(self) -> u8 {
        self.zone
    }

    /// This slot's position in the flat numbering shared by both ends.
    ///
    /// The `saturating_add` cannot saturate: the largest
    /// [`first_slot`](SnapLayoutPreset::first_slot) is 16 and the largest zone
    /// within it is 5. It is written that way so the function is total without
    /// an unreachable panicking branch, and
    /// `an_index_and_the_slot_it_names_are_inverses` proves the arithmetic is
    /// the inverse of [`from_index`](Self::from_index) rather than merely
    /// plausible.
    #[must_use]
    pub const fn index(self) -> u8 {
        self.preset.first_slot().saturating_add(self.zone)
    }

    /// The slot an index names, or `None` if it names none of them.
    #[must_use]
    pub const fn from_index(index: u8) -> Option<Self> {
        let preset = match index {
            0..=1 => SnapLayoutPreset::TwoEqualHalves,
            2..=4 => SnapLayoutPreset::ThreeColumns,
            5..=6 => SnapLayoutPreset::TwoThirdsLeft,
            7..=8 => SnapLayoutPreset::TwoThirdsRight,
            9..=12 => SnapLayoutPreset::FourQuadrants,
            13..=15 => SnapLayoutPreset::ThreeLeftTwoRight,
            16..=21 => SnapLayoutPreset::SixGrid,
            _ => return None,
        };
        // Cannot underflow: every arm above begins at its preset's own
        // `first_slot`. Saturating rather than checked for `index`'s reason.
        Some(Self {
            preset,
            zone: index.saturating_sub(preset.first_slot()),
        })
    }

    /// Every slot there is, in wire order.
    ///
    /// Derived from [`from_index`](Self::from_index) rather than written out a
    /// second time, so the list cannot drift from the numbering it is supposed
    /// to enumerate — which is the failure a hand-written table invites and the
    /// reason the tests below can trust this one.
    pub fn all() -> impl Iterator<Item = Self> {
        (0..Self::COUNT).filter_map(Self::from_index)
    }

    /// Where this slot lands on a display whose usable region is `area`.
    ///
    /// The one call the compositor makes: it is the only party that knows its
    /// own bounds, and the shell is not allowed to compute a rectangle. `None`
    /// only if a preset's [`zone_count`](SnapLayoutPreset::zone_count) ever
    /// disagreed with the zones it actually builds, which
    /// `zone_counts_match_the_zones_built` forbids — but it is reported rather
    /// than asserted, because ignoring a request that makes no sense is the
    /// right thing for a value that arrived over a wire.
    #[must_use]
    pub fn rect(self, area: WorkArea) -> Option<SnapZone> {
        self.preset
            .build(area)
            .zones
            .into_iter()
            .nth(usize::from(self.zone))
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

    // ======================================================================
    // SnapSlot
    //
    // Four tables have to agree for a tiling request to mean the same thing at
    // both ends: each preset's `zones_with_gap` arm, its `zone_count`, its
    // `first_slot` offset, and `from_index`'s ranges. Nothing in the compiler
    // relates them, and a disagreement is silent — a window lands in a zone
    // other than the one the user aimed at, or a byte the sender considered
    // valid is refused. The tests below are what relate them.
    // ======================================================================

    #[test]
    fn zone_counts_match_the_zones_built() {
        // `zone_count` exists because a wire byte has to be validated with no
        // work area in hand, so it restates something `build` already knows.
        // Restating it is exactly how the two come apart.
        for &preset in SnapLayoutPreset::all() {
            assert_eq!(
                usize::from(preset.zone_count()),
                preset.build(SCREEN).zones.len(),
                "{preset:?} says it has {} zones and builds a different number",
                preset.zone_count()
            );
        }
    }

    #[test]
    fn count_is_the_sum_of_every_presets_zones() {
        let summed: u8 = SnapLayoutPreset::all().iter().map(|p| p.zone_count()).sum();
        assert_eq!(
            summed,
            SnapSlot::COUNT,
            "COUNT is {} but the presets between them have {summed} zones",
            SnapSlot::COUNT
        );
    }

    #[test]
    fn the_presets_slot_ranges_are_end_to_end_with_no_gap_or_overlap() {
        // `first_slot` is a hand-written offset table. If one entry is off by
        // one, two presets share a slot — so a request to tile into the left
        // half arrives as a request to tile into the left third, which is a
        // wrong window position rather than an error anyone would notice.
        let mut expected = 0u8;
        for &preset in SnapLayoutPreset::all() {
            assert_eq!(
                preset.first_slot(),
                expected,
                "{preset:?} starts at slot {} but the presets before it end at {expected}",
                preset.first_slot()
            );
            expected += preset.zone_count();
        }
        assert_eq!(expected, SnapSlot::COUNT);
    }

    #[test]
    fn an_index_and_the_slot_it_names_are_inverses() {
        // Both directions, over the whole byte, because `index` and
        // `from_index` are separate pieces of arithmetic over the same table
        // and nothing but this makes them agree.
        for index in 0..=u8::MAX {
            match SnapSlot::from_index(index) {
                Some(slot) => {
                    assert!(
                        index < SnapSlot::COUNT,
                        "{index} decoded but is out of range"
                    );
                    assert_eq!(
                        slot.index(),
                        index,
                        "{slot:?} came from {index} but indexes back to {}",
                        slot.index()
                    );
                }
                None => assert!(
                    index >= SnapSlot::COUNT,
                    "{index} is inside the range but names no slot"
                ),
            }
        }

        for slot in SnapSlot::all() {
            assert_eq!(SnapSlot::from_index(slot.index()), Some(slot));
        }
    }

    #[test]
    fn every_slot_names_a_zone_its_preset_actually_has() {
        for slot in SnapSlot::all() {
            assert!(
                slot.zone() < slot.preset().zone_count(),
                "{slot:?} names zone {} of a layout with {} of them",
                slot.zone(),
                slot.preset().zone_count()
            );
            assert_eq!(SnapSlot::new(slot.preset(), slot.zone()), Some(slot));
        }
    }

    #[test]
    fn a_zone_a_preset_does_not_have_is_not_a_slot() {
        for &preset in SnapLayoutPreset::all() {
            assert_eq!(
                SnapSlot::new(preset, preset.zone_count()),
                None,
                "{preset:?} accepted one zone more than it has"
            );
            assert_eq!(SnapSlot::new(preset, u8::MAX), None);
        }
    }

    #[test]
    fn all_visits_every_slot_exactly_once() {
        let slots: Vec<SnapSlot> = SnapSlot::all().collect();
        assert_eq!(slots.len(), usize::from(SnapSlot::COUNT));
        for slot in &slots {
            assert_eq!(
                slots.iter().filter(|s| *s == slot).count(),
                1,
                "{slot:?} appears more than once"
            );
        }
    }

    #[test]
    fn a_slots_rectangle_is_the_zone_of_that_number_in_its_layout() {
        // The compositor's only call. If `rect` reached into the wrong layout,
        // or counted from the wrong end, the window would land somewhere
        // plausible rather than nowhere — so compare against the layout the
        // shell's own picker draws, zone for zone.
        for &preset in SnapLayoutPreset::all() {
            let drawn = preset.build(TOP_BAR_DESK).zones;
            for (index, zone) in drawn.iter().enumerate() {
                let slot = SnapSlot::new(preset, u8::try_from(index).expect("small"))
                    .expect("a zone the layout built is a zone the layout has");
                assert_eq!(
                    slot.rect(TOP_BAR_DESK).as_ref(),
                    Some(zone),
                    "{preset:?} zone {index} resolves to a different rectangle than it is drawn at"
                );
            }
        }
    }

    #[test]
    fn a_rectangle_is_resolved_against_the_display_it_is_asked_about() {
        // The whole point of sending a slot rather than a rectangle: the same
        // request means different pixels on different displays, and the sender
        // does not have to know which.
        let slot = SnapSlot::new(SnapLayoutPreset::TwoEqualHalves, 1).expect("right half exists");
        let wide = slot.rect(SCREEN).expect("resolves");
        let inset = slot.rect(TOP_BAR_DESK).expect("resolves");
        assert!(
            inset.y > wide.y,
            "the inset work area's zone starts at {} , the same as the full screen's {}",
            inset.y,
            wide.y
        );
        assert!(inset.height < wide.height);
    }
}
