//! Window snap zones -- Windows 11-style snap layouts for the desktop shell.
//!
//! Provides a zone-based window snapping system that the user invokes through
//! the zone picker at the top edge. Each [`SnapLayout`] defines a set of
//! non-overlapping [`SnapZone`]s covering the work area; the [`SnapManager`]
//! tracks the active layout, performs hit-testing, renders overlays, and turns
//! a chosen zone into the [`SnapSlot`] a `ShellControlAction` carries to ask
//! the compositor to tile the window there.
//!
//! The *other* way to reach the same layouts — drag a window to a screen edge
//! and drop it — is not here and never fires through this module. It belongs
//! to the compositor, which holds the drag grab and can answer on every motion
//! event without a socket round trip inside the part of the gesture the user
//! watches. Its rules are [`guiremote::zones::drop_at`].
//!
//! # What is here, and what is in `guiremote`
//!
//! The *shapes* -- [`SnapLayoutPreset`], the [`SnapZone`]s it builds over a
//! [`WorkArea`] -- live in [`guiremote::zones`] and are re-exported below.
//! They had to move: the shell cannot place a window, so tiling one means
//! asking the compositor for a named zone, and a named zone means nothing
//! unless both ends compute the same rectangle for it. See
//! `design-decisions.md` for why the protocol crate rather than a crate of
//! their own.
//!
//! What stays here is everything that is the *shell's* rather than the
//! protocol's or the compositor's: the zone overlay and layout picker (render
//! trees), the hit-testing over them, and the translation from "the user
//! clicked that zone" to "ask for that slot".
//!
//! # The shell names a tile; it never places one
//!
//! Nothing in this module returns a rectangle to act on, and that is the
//! point. The shell has no window geometry — `apply_window_list` overwrites
//! whatever it thought it knew on the compositor's next snapshot — so a shell
//! that computed a placement would be computing a number it cannot use and the
//! compositor will not read. What it produces instead is a [`SnapSlot`] inside
//! a `ShellControlAction`, and the compositor resolves it against the display
//! bounds only the compositor has.
//!
//! # Usage from the desktop shell
//!
//! ```ignore
//! let mut snap = SnapManager::new(WorkArea::new(0.0, 0.0, 1920.0, 1032.0));
//! snap.set_layout(SnapLayoutPreset::TwoEqualHalves);
//!
//! // While the user is dragging a window:
//! if cursor_near_top_edge {
//!     snap.show_overlay();
//! }
//! if let Some(zone) = snap.hit_test(cursor_x, cursor_y) {
//!     let highlight = snap.render_zone_highlight(zone.id);
//!     // draw highlight commands
//! }
//!
//! // On drop: ask the compositor to tile it there.
//! if let Some(slot) = snap.slot_for_zone(zone.id) {
//!     send(WindowRequest::new(window_id, ShellControlAction::SnapToZone(slot)));
//! }
//! ```
//!
//! # Colour
//!
//! The three renderers take a [`Palette`] rather than reading a `theme`
//! submodule of hand-written Mocha constants, which is part 2 of
//! `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`. This
//! module is the sharpest illustration of that defect in the tree, because the
//! answer had already been written down *for it*: [`Palette::selection_fill`],
//! [`Palette::highlight_fill`] and [`Palette::selection_border`] exist, the
//! last one's doc comment says "the outline of a selection **or of a
//! highlighted snap zone**", and their alphas are 50 / 90 / 150 against this
//! module's 50 / 90 / 160. The palette was derived from these constants and
//! nobody wired it back, so the copy sat here and drifted by ten in one alpha.
//! Four judgements are worth stating.
//!
//! **1. A snap zone is a selection, so it follows the accent.** The zone
//! previews, the hovered zone and the active preset's marker were all Mocha
//! `blue`; they are now the accent, because "the one you are about to pick" is
//! what an accent is for. This is the opposite answer from
//! [`screen_capture`](crate::screen_capture), whose red/yellow/green transport
//! buttons are a code the user reads and must *not* follow the accent — same
//! roles, opposite conclusions, and the difference is whether the colour
//! carries meaning of its own or only says "this one".
//!
//! **2. The labels follow the scrim, not the mode.** This is the one place a
//! naive conversion would have introduced a bug. `render_overlay` lays a scrim
//! over the whole work area first, and a scrim is black in both modes
//! (§525 decision 3) — so whatever the theme, the zone labels are drawn on a
//! *darkened* backdrop. Turning their `MOCHA_TEXT` into `p.text` would put
//! Latte's `#4C4F69` on that in light mode and lose the label. They are
//! [`readable_on`] of [`Palette::scrim`] instead, which is near-white in both
//! modes because a scrim's RGB is `(0, 0, 0)` in both. The hovered zone's
//! label, previously a raw `Color::WHITE`, is the same value by the same
//! argument rather than by coincidence.
//!
//! **3. The picker is a panel and obeys the transparency setting.** Its
//! background and hover rung were `rgba(30, 30, 46, 230)` and
//! `rgba(69, 71, 90, 200)` — `base` and `surface1` with an alpha frozen into
//! the literal, which is a *setting* written down as a colour. They are
//! [`Palette::panel_bg`] and [`Palette::panel_hover`], so a user who turns
//! transparency off gets an opaque picker.
//!
//! **4. The scrim was tinted and should not have been.** It was
//! `rgba(30, 30, 46, 140)` — Mocha `base` at an alpha. The alpha matches
//! [`Palette::scrim`] exactly; the tint is the bug, and it is the one the
//! `scrim` doc comment was written to prevent: Latte's base is `#EFF1F5`, so a
//! base-tinted scrim in light mode would have *lightened* the desktop it was
//! meant to push back.
//!
//! **5. A hue against a rung, never a hue against a hue.** The picker's
//! thumbnails drew the active preset's mini-zones in `blue` and every other
//! preset's in `lavender`. Once judgement 1 makes the active one the accent,
//! that pair stops distinguishing anything for a user whose accent *is*
//! lavender — and [`AccentColor::Lavender`](appearance::AccentColor::Lavender)
//! is one of the fourteen the settings page offers. The inactive presets are
//! [`Palette::overlay0`] instead: a surface rung cannot collide with any
//! accent, so the thumbnail says which layout is active under every theme the
//! user can choose rather than under thirteen of fourteen. This one is not a
//! conversion at all — it is a bug the conversion exposed, because it is
//! invisible while the accent is hard-coded to blue.

use appearance::{Palette, readable_on};
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;

use std::num::NonZeroUsize;

// Re-exported rather than merely imported so that `snap::SnapZone` keeps
// resolving for this crate's callers and tests. The move is a change of *home*,
// not of vocabulary: the shell still talks about zones and presets in exactly
// the words it did.
pub use guiremote::zones::{
    SnapLayout, SnapLayoutPreset, SnapSlot, SnapZone, WorkArea, ZONE_GAP, ZoneId,
};

// ============================================================================
// Constants
// ============================================================================

/// Distance from top of screen to trigger the zone layout picker.
const TOP_PICKER_THRESHOLD: f32 = 16.0;

/// Width of the layout picker popup.
const PICKER_WIDTH: f32 = 340.0;
/// Padding inside the picker.
const PICKER_PADDING: f32 = 12.0;
/// Size of a single layout thumbnail in the picker.
const THUMB_SIZE: f32 = 72.0;
/// Gap between thumbnails.
const THUMB_GAP: f32 = 10.0;

// ============================================================================
// SnapManager
// ============================================================================

/// Main snap-zone manager. Owns the active layout and overlay state.
pub struct SnapManager {
    /// The rectangle zones tile — the screen minus the taskbar.
    area: WorkArea,
    /// Current layout preset.
    active_preset: SnapLayoutPreset,
    /// Current built layout.
    layout: SnapLayout,
    /// Whether the zone overlay is visible (e.g. during a drag).
    overlay_visible: bool,
    /// Whether the three-way layout picker popup is visible.
    picker_visible: bool,
    /// Which preset in the picker is currently hovered (index into
    /// `SnapLayoutPreset::all()`), or `None`.
    picker_hover_index: Option<usize>,
    /// Which zone the cursor is over, or `None`.
    ///
    /// Held here rather than on the shell because it is derived from `layout`,
    /// which is rebuilt whenever the work area changes; a hover kept beside a
    /// layout it was not measured against is a highlight drawn over a zone that
    /// has moved.
    hovered_zone: Option<ZoneId>,
}

impl SnapManager {
    /// Create a new snap manager tiling the given work area.
    pub fn new(area: WorkArea) -> Self {
        let active_preset = SnapLayoutPreset::TwoEqualHalves;
        let layout = active_preset.build(area);
        Self {
            area,
            active_preset,
            layout,
            overlay_visible: false,
            picker_visible: false,
            picker_hover_index: None,
            hovered_zone: None,
        }
    }

    /// The work area zones are tiled over.
    pub fn work_area(&self) -> WorkArea {
        self.area
    }

    /// Currently active layout preset.
    pub fn active_preset(&self) -> SnapLayoutPreset {
        self.active_preset
    }

    /// Reference to the current layout.
    pub fn layout(&self) -> &SnapLayout {
        &self.layout
    }

    /// Whether the overlay is currently visible.
    pub fn is_overlay_visible(&self) -> bool {
        self.overlay_visible
    }

    /// Whether the layout picker popup is showing.
    pub fn is_picker_visible(&self) -> bool {
        self.picker_visible
    }

    /// The zone the cursor is over, if any.
    #[must_use]
    pub fn hovered_zone(&self) -> Option<ZoneId> {
        self.hovered_zone
    }

    // ======================================================================
    // Layout management
    // ======================================================================

    /// Switch to a different layout preset, rebuilding zones.
    ///
    /// Drops the hovered zone: it was an index into the layout being replaced,
    /// and six of the seven presets have a different number of zones, so
    /// keeping it would light a highlight the cursor is not over.
    pub fn set_layout(&mut self, preset: SnapLayoutPreset) {
        self.active_preset = preset;
        self.layout = preset.build(self.area);
        self.hovered_zone = None;
    }

    /// Recalculate zones after the work area changes.
    ///
    /// That is not only a screen resize: it also happens when the taskbar is
    /// resized, hidden or auto-hidden, which changes the height available
    /// without the display changing at all.
    pub fn set_work_area(&mut self, area: WorkArea) {
        self.area = area;
        self.layout = self.active_preset.build(area);
        // Same reasoning as `set_layout`: the zones just moved, so the last
        // cursor position no longer says which one it is over.
        self.hovered_zone = None;
    }

    // ======================================================================
    // Overlay visibility
    // ======================================================================

    /// Show the snap zone overlay (called when a window drag enters
    /// a trigger region).
    pub fn show_overlay(&mut self) {
        self.overlay_visible = true;
    }

    /// Hide the snap zone overlay.
    pub fn hide_overlay(&mut self) {
        self.overlay_visible = false;
        self.picker_visible = false;
        self.picker_hover_index = None;
        self.hovered_zone = None;
    }

    /// Show the three-way zone layout picker (hover near top while
    /// dragging).
    pub fn show_picker(&mut self) {
        self.picker_visible = true;
    }

    /// Hide the picker without selecting a layout.
    pub fn hide_picker(&mut self) {
        self.picker_visible = false;
        self.picker_hover_index = None;
    }

    // ======================================================================
    // Hit testing
    // ======================================================================

    /// Find which zone the point `(x, y)` falls within.
    /// Returns `None` if the cursor is outside all zones.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<&SnapZone> {
        self.layout.zones.iter().find(|z| z.contains(x, y))
    }

    /// Find the zone by id.
    pub fn zone_by_id(&self, zone_id: ZoneId) -> Option<&SnapZone> {
        self.layout.zones.iter().find(|z| z.id == zone_id)
    }

    /// Detect whether the cursor is in the top-edge region that
    /// triggers the layout picker.
    ///
    /// Relative to the work area's top, not the screen's. They coincide with a
    /// bottom taskbar and stop coinciding the moment the bar moves to the top,
    /// where the trigger band would otherwise sit behind it.
    ///
    /// Bounded below as well as above, for the same reason
    /// [`guiremote::zones::edge_at`] is: an unbounded
    /// `cursor_y < top + THRESHOLD` also matches everything *above* the work
    /// area, which is exactly the strip the taskbar occupies.
    pub fn is_in_picker_trigger(&self, _cursor_x: f32, cursor_y: f32) -> bool {
        cursor_y >= self.area.y && cursor_y < self.area.y + TOP_PICKER_THRESHOLD
    }

    /// Follow the cursor: update both the hovered zone and the hovered picker
    /// thumbnail. `cursor_x` / `cursor_y` are absolute screen coordinates.
    ///
    /// One door for both, so that a caller cannot update one and forget the
    /// other and leave a highlight lit under a cursor that has left it. The
    /// picker wins where the two overlap, because the picker is drawn over the
    /// zones: highlighting the zone *behind* an open panel would promise a
    /// placement that the press is going to spend selecting a layout instead.
    pub fn update_hover(&mut self, cursor_x: f32, cursor_y: f32) {
        self.update_picker_hover(cursor_x, cursor_y);
        self.hovered_zone = if self.picker_hit(cursor_x, cursor_y) {
            None
        } else {
            self.hit_test(cursor_x, cursor_y).map(|z| z.id)
        };
    }

    /// Update the picker hover state. `cursor_x` / `cursor_y` are
    /// absolute screen coordinates.
    fn update_picker_hover(&mut self, cursor_x: f32, cursor_y: f32) {
        if !self.picker_visible {
            self.picker_hover_index = None;
            return;
        }

        for i in 0..SnapLayoutPreset::all().len() {
            let (ix, iy) = self.thumb_origin(i);

            if cursor_x >= ix
                && cursor_x < ix + THUMB_SIZE
                && cursor_y >= iy
                && cursor_y < iy + THUMB_SIZE
            {
                self.picker_hover_index = Some(i);
                return;
            }
        }
        self.picker_hover_index = None;
    }

    /// If the picker is showing and the user clicks, select the hovered
    /// layout. Returns `true` if a selection was made.
    pub fn picker_select(&mut self) -> bool {
        if let Some(idx) = self.picker_hover_index {
            let presets = SnapLayoutPreset::all();
            if let Some(&preset) = presets.get(idx) {
                self.set_layout(preset);
                self.picker_visible = false;
                self.picker_hover_index = None;
                return true;
            }
        }
        false
    }

    // ======================================================================
    // Choosing a tile
    // ======================================================================

    /// The [`SnapSlot`] naming a zone of the active layout, for the request the
    /// shell will send.
    ///
    /// A *name*, not a rectangle. This method used to return the zone's
    /// `(x, y, width, height)` and the shell had nothing to do with it: the
    /// shell cannot move a window, so the geometry was computed, returned and
    /// dropped, while the window stayed where it was. The rectangle is the
    /// compositor's to work out, from bounds only it knows, which is why what
    /// crosses the wire is the slot.
    ///
    /// Returns `None` for a zone id the active layout does not have.
    #[must_use]
    pub fn slot_for_zone(&self, zone_id: ZoneId) -> Option<SnapSlot> {
        let zone = u8::try_from(zone_id).ok()?;
        SnapSlot::new(self.active_preset, zone)
    }

    // ======================================================================
    // Rendering -- overlay
    // ======================================================================

    /// Render the full snap zone overlay (semi-transparent zone
    /// previews over the entire screen).
    pub fn render_overlay(&self, p: &Palette) -> Vec<RenderCommand> {
        if !self.overlay_visible {
            return Vec::new();
        }

        // Saturating for the same reason as in `render_picker`: a capacity
        // hint that panics is worse than one that is merely wrong.
        let mut cmds =
            Vec::with_capacity(self.layout.zones.len().saturating_mul(3).saturating_add(1));

        // Scrim behind everything — over the work area only, so the taskbar
        // is not dimmed along with the desktop it sits beside.
        cmds.push(RenderCommand::FillRect {
            x: self.area.x,
            y: self.area.y,
            width: self.area.width,
            height: self.area.height,
            // Judgement 4: black, not a tinted base. Latte's base is
            // `#EFF1F5`, so the constant this replaces would have lightened
            // the desktop in light mode instead of pushing it back.
            color: p.scrim(),
            corner_radii: CornerRadii::ZERO,
        });

        for zone in &self.layout.zones {
            // Zone fill.
            cmds.push(RenderCommand::FillRect {
                x: zone.x,
                y: zone.y,
                width: zone.width,
                height: zone.height,
                // Judgement 1: a zone at rest is a selection at rest.
                color: p.selection_fill(),
                corner_radii: CornerRadii::all(8.0),
            });

            // Zone border.
            cmds.push(RenderCommand::StrokeRect {
                x: zone.x,
                y: zone.y,
                width: zone.width,
                height: zone.height,
                // The helper whose doc comment already named this site. Its
                // alpha is 150 where the deleted constant's was 160 — the
                // drift closing, not a change of intent.
                color: p.selection_border(),
                line_width: 2.0,
                corner_radii: CornerRadii::all(8.0),
            });

            // Zone label centred.
            let (cx, cy) = zone.center();
            cmds.push(RenderCommand::Text {
                x: text::center_x(zone.label, cx, 13.0, FontWeightHint::Regular),
                y: cy - 7.0,
                text: zone.label.to_string(),
                // Judgement 2: the label sits on the scrim above, which is
                // black in both modes, so its ink is too. `p.text` here would
                // be dark-on-dark under the light theme.
                color: readable_on(p.scrim()),
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(zone.width - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        cmds
    }

    /// Render a highlight for a single zone (the one the cursor is
    /// hovering over).
    pub fn render_zone_highlight(&self, p: &Palette, zone_id: ZoneId) -> Vec<RenderCommand> {
        let zone = match self.zone_by_id(zone_id) {
            Some(z) => z,
            None => return Vec::new(),
        };

        let mut cmds = Vec::with_capacity(3);

        // Highlighted fill.
        cmds.push(RenderCommand::FillRect {
            x: zone.x,
            y: zone.y,
            width: zone.width,
            height: zone.height,
            // Judgement 1, one rung louder than the zones at rest.
            color: p.highlight_fill(),
            corner_radii: CornerRadii::all(8.0),
        });

        // Border.
        cmds.push(RenderCommand::StrokeRect {
            x: zone.x,
            y: zone.y,
            width: zone.width,
            height: zone.height,
            // The accent at full strength rather than `selection_border`'s
            // wash: this is the one zone the drop will land in, and it has to
            // out-read the eight at rest that are already wearing that wash.
            color: p.accent,
            line_width: 3.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Label.
        let (cx, cy) = zone.center();
        cmds.push(RenderCommand::Text {
            x: text::center_x(zone.label, cx, 14.0, FontWeightHint::Bold),
            y: cy - 8.0,
            text: zone.label.to_string(),
            // Judgement 2 again, and the reason it is stated as a rule: this
            // was a raw `Color::WHITE`, which is the right *value* reached by
            // the wrong argument. It agrees with the labels beneath it now
            // because both are read off the same scrim, not by coincidence.
            color: readable_on(p.scrim()),
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(zone.width - 16.0),
            overflow: TextOverflow::Ellipsis,
        });

        cmds
    }

    // ======================================================================
    // Rendering -- layout picker popup
    // ======================================================================

    /// Top-left corner of the picker popup, centred at the top of the work
    /// area — the same rectangle the trigger band is measured against, so the
    /// popup appears where the gesture that summons it happens.
    fn picker_origin(&self) -> (f32, f32) {
        let px = self.area.x + (self.area.width - PICKER_WIDTH) / 2.0;
        let py = self.area.y + TOP_PICKER_THRESHOLD + 4.0;
        (px, py)
    }

    /// How many thumbnail items fit in one row.
    ///
    /// `NonZeroUsize` rather than `usize`: the `.max(1)` below already made
    /// zero impossible, but only the type says so, and both callers divide by
    /// this. A guarantee the compiler can see is worth more than one a reader
    /// has to go and check.
    fn picker_items_per_row(&self) -> NonZeroUsize {
        let usable = PICKER_WIDTH - 2.0 * PICKER_PADDING + THUMB_GAP;
        NonZeroUsize::new((usable / (THUMB_SIZE + THUMB_GAP)) as usize).unwrap_or(NonZeroUsize::MIN)
    }

    /// Top-left corner of the `index`th thumbnail in the picker grid.
    ///
    /// Shared by `update_picker_hover` and `render_picker`. They previously
    /// each computed this grid from scratch, with the two copies four hundred
    /// lines apart — an arrangement in which editing one padding constant
    /// gives you a picker that highlights one thumbnail and selects another,
    /// and no test that would notice.
    fn thumb_origin(&self, index: usize) -> (f32, f32) {
        let (px, py) = self.picker_origin();
        let per_row = self.picker_items_per_row();
        // `usize % NonZeroUsize` and `usize / NonZeroUsize`: infallible by
        // type, so there is no divide-by-zero branch to write or to test.
        let col = index % per_row;
        let row = index / per_row;
        (
            px + PICKER_PADDING + col as f32 * (THUMB_SIZE + THUMB_GAP),
            py + PICKER_PADDING + 24.0 + row as f32 * (THUMB_SIZE + THUMB_GAP),
        )
    }

    /// The picker popup's rectangle, as `(x, y, width, height)`.
    ///
    /// Answered whether or not the picker is showing, because a rectangle is a
    /// fact about the grid and not about its visibility; the callers that care
    /// ask [`is_picker_visible`](Self::is_picker_visible) first.
    ///
    /// Shared by [`render_picker`](Self::render_picker) and by the shell's hit
    /// test, for the reason [`thumb_origin`](Self::thumb_origin) gives: a panel
    /// drawn from one height and clicked against another is a panel whose lower
    /// rows either swallow clicks aimed past them or leak clicks aimed at them,
    /// and nothing about either failure says which of the two copies is wrong.
    #[must_use]
    pub fn picker_rect(&self) -> (f32, f32, f32, f32) {
        let (px, py) = self.picker_origin();
        let rows = SnapLayoutPreset::all()
            .len()
            .div_ceil(self.picker_items_per_row().get());
        let height =
            PICKER_PADDING * 2.0 + 24.0 + rows as f32 * (THUMB_SIZE + THUMB_GAP) - THUMB_GAP;
        (px, py, PICKER_WIDTH, height)
    }

    /// Where the picker draws `preset`'s thumbnail, as `(x, y, size)`, or
    /// `None` if `preset` is not one the picker offers.
    ///
    /// Derived from [`thumb_origin`](Self::thumb_origin) — the same grid
    /// [`render_picker`](Self::render_picker) and `update_picker_hover` walk —
    /// so a caller aiming at a thumbnail cannot aim at a rectangle the picker
    /// never drew.
    #[must_use]
    pub fn thumbnail_rect(&self, preset: SnapLayoutPreset) -> Option<(f32, f32, f32)> {
        let index = SnapLayoutPreset::all().iter().position(|&p| p == preset)?;
        let (x, y) = self.thumb_origin(index);
        Some((x, y, THUMB_SIZE))
    }

    /// Whether `(x, y)` lands on the open picker popup.
    ///
    /// False when the picker is hidden: a point cannot be on a panel that is
    /// not there, and answering otherwise would let the strip of screen the
    /// picker *would* occupy swallow clicks meant for the zone beneath it.
    #[must_use]
    pub fn picker_hit(&self, x: f32, y: f32) -> bool {
        if !self.picker_visible {
            return false;
        }
        let (px, py, w, h) = self.picker_rect();
        x >= px && x < px + w && y >= py && y < py + h
    }

    /// Render the layout picker popup.
    pub fn render_picker(&self, p: &Palette) -> Vec<RenderCommand> {
        if !self.picker_visible {
            return Vec::new();
        }

        let (px, py, _, picker_h) = self.picker_rect();
        let presets = SnapLayoutPreset::all();

        // Saturating, because this is a capacity hint: a wrong answer costs a
        // reallocation, and an overflow panic on a hint would be absurd.
        let mut cmds = Vec::with_capacity(presets.len().saturating_mul(8).saturating_add(4));

        // Shadow.
        cmds.push(RenderCommand::BoxShadow {
            x: px,
            y: py,
            width: PICKER_WIDTH,
            height: picker_h,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 16.0,
            spread: 0.0,
            // Left as a black at an alpha rather than routed through the
            // palette: a drop shadow is an absence of light, so it does not
            // flip with the mode (§525 decision 3). `p.shadow()` is the same
            // black at 120; this one is deliberately lighter because the
            // picker is a small popup rather than a window.
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::all(10.0),
        });

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: px,
            y: py,
            width: PICKER_WIDTH,
            height: picker_h,
            // Judgement 3: the transparency setting, not a frozen 230.
            color: p.panel_bg(),
            corner_radii: CornerRadii::all(10.0),
        });

        // Border.
        cmds.push(RenderCommand::StrokeRect {
            x: px,
            y: py,
            width: PICKER_WIDTH,
            height: picker_h,
            color: p.surface0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(10.0),
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: px + PICKER_PADDING,
            y: py + PICKER_PADDING,
            text: "Snap Layout".into(),
            // Kept as a hue rather than promoted to the accent: the title is
            // decoration on a panel, and an accented title would compete with
            // the accented thumbnail below it that actually means something.
            color: p.lavender,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(PICKER_WIDTH - 2.0 * PICKER_PADDING),
            overflow: TextOverflow::Ellipsis,
        });

        // Thumbnails.
        for (i, &preset) in presets.iter().enumerate() {
            let (ix, iy) = self.thumb_origin(i);

            // Hover highlight.
            if self.picker_hover_index == Some(i) {
                cmds.push(RenderCommand::FillRect {
                    x: ix - 2.0,
                    y: iy - 2.0,
                    width: THUMB_SIZE + 4.0,
                    height: THUMB_SIZE + 4.0,
                    // Judgement 3: `surface1` at the transparency setting, not
                    // at a frozen 200.
                    color: p.panel_hover(),
                    corner_radii: CornerRadii::all(6.0),
                });
            }

            // Thumbnail background.
            cmds.push(RenderCommand::FillRect {
                x: ix,
                y: iy,
                width: THUMB_SIZE,
                height: THUMB_SIZE,
                color: p.surface0,
                corner_radii: CornerRadii::all(4.0),
            });

            // Mini-zone rectangles inside the thumbnail. Built at the
            // thumbnail's own origin rather than at zero and offset by the
            // caller, so the thumbnail and the real overlay go through the
            // same origin handling.
            let mini_layout = preset.build(WorkArea::new(
                ix + 4.0,
                iy + 4.0,
                THUMB_SIZE - 8.0,
                THUMB_SIZE - 8.0,
            ));
            for zone in &mini_layout.zones {
                cmds.push(RenderCommand::FillRect {
                    x: zone.x,
                    y: zone.y,
                    width: zone.width,
                    height: zone.height,
                    // Judgement 5: the *active* preset is accented, and the
                    // rest drop to a surface rung rather than to a second hue.
                    // They used to be blue and lavender, which is a pair that
                    // stops distinguishing anything the moment a user picks
                    // lavender as their accent — and lavender is one of the
                    // accents the settings page offers. A hue against a rung
                    // cannot collide with any accent.
                    color: if self.active_preset == preset {
                        p.accent
                    } else {
                        p.overlay0
                    },
                    corner_radii: CornerRadii::all(2.0),
                });
            }

            // Active indicator.
            if self.active_preset == preset {
                cmds.push(RenderCommand::StrokeRect {
                    x: ix,
                    y: iy,
                    width: THUMB_SIZE,
                    height: THUMB_SIZE,
                    color: p.accent,
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
        }

        cmds
    }
}

// ============================================================================
// Tests
// ============================================================================

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
    use crate::palette_check::assert_drawn_from;
    use appearance::AccentColor;

    /// A palette for `light` whose accent is off-palette.
    ///
    /// The stock accent **is** `blue`, so a fixture built with
    /// `Palette::for_mode` cannot tell "this site follows the accent" from
    /// "this site is hard-coded blue" — which is the exact defect this module
    /// was converted to remove, and the one judgement 5 turned out to be
    /// hiding. Magenta is in neither Mocha nor Latte, and the guard below says
    /// so rather than trusting that it stays true.
    fn accented(light: bool) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = Color::from_hex(0xFF00FF);
        assert!(
            !p.roles()
                .iter()
                .any(|(n, r)| *n != "accent" && r.r == 255 && r.g == 0 && r.b == 255),
            "the fixture's accent collided with a role, so accent tests would \
             pass for the wrong reason"
        );
        p
    }

    /// A 1080p screen with nothing subtracted.
    ///
    /// Most tests here are about proportions — that two halves are equal, that
    /// six zones do not overlap — and those hold at any origin, so they use
    /// this. The tests that are specifically about the *origin* use [`DESK`]
    /// and [`TOP_BAR_DESK`] below, because a work area that starts at (0, 0)
    /// and fills the screen is exactly the case in which the defect this file
    /// was fixed for is invisible.
    const SCREEN: WorkArea = WorkArea::whole_screen(1920.0, 1080.0);

    /// The same screen with a 48 px taskbar along the bottom: origin still at
    /// (0, 0), but 48 px shorter.
    const DESK: WorkArea = WorkArea::new(0.0, 0.0, 1920.0, 1032.0);

    /// The same screen with the taskbar along the *top* — an arrangement in
    /// which the work area's origin is not (0, 0), and therefore one that can
    /// catch a zone builder that ignores it.
    const TOP_BAR_DESK: WorkArea = WorkArea::new(0.0, 48.0, 1920.0, 1032.0);

    // --- zone label centring ---

    #[test]
    fn a_zone_label_is_centred_on_its_zone() {
        // These were centred by subtracting 3.5 px per *byte* — half a guessed
        // seven-pixel character — so "Left Half" sat off-centre and a localised
        // zone name sat off the zone entirely.
        for (label, size, weight) in [
            ("Left Half", 13.0, FontWeightHint::Regular),
            ("Top Right Quarter", 13.0, FontWeightHint::Regular),
            ("Maximise", 14.0, FontWeightHint::Bold),
            ("Linke Hälfte", 14.0, FontWeightHint::Bold),
        ] {
            let centre = 640.0;
            let x = guitk::text::center_x(label, centre, size, weight);
            let w = guitk::text::measure(label, size, weight);
            assert!(
                (x + w / 2.0 - centre).abs() < 0.01,
                "{label:?} is not centred: spans {x}..{}",
                x + w
            );
        }
    }

    #[test]
    fn the_picker_trigger_follows_the_work_areas_top() {
        let mgr = SnapManager::new(TOP_BAR_DESK);
        assert!(mgr.is_in_picker_trigger(960.0, TOP_BAR_DESK.y + 5.0));
        assert!(
            !mgr.is_in_picker_trigger(960.0, TOP_BAR_DESK.y + 200.0),
            "200 px into the desktop is not the trigger band"
        );
        assert!(
            !mgr.is_in_picker_trigger(960.0, 5.0),
            "y=5 is inside a top taskbar, not the desktop's top edge"
        );
    }

    // ======================================================================
    // SnapManager -- construction & basic state
    // ======================================================================

    fn make_manager() -> SnapManager {
        SnapManager::new(SCREEN)
    }

    #[test]
    fn manager_starts_with_default_layout() {
        let mgr = make_manager();
        assert_eq!(mgr.active_preset(), SnapLayoutPreset::TwoEqualHalves);
        assert_eq!(mgr.layout().zones.len(), 2);
    }

    #[test]
    fn manager_overlay_initially_hidden() {
        let mgr = make_manager();
        assert!(!mgr.is_overlay_visible());
        assert!(!mgr.is_picker_visible());
    }

    #[test]
    fn show_and_hide_overlay() {
        let mut mgr = make_manager();
        mgr.show_overlay();
        assert!(mgr.is_overlay_visible());
        mgr.hide_overlay();
        assert!(!mgr.is_overlay_visible());
    }

    // ======================================================================
    // SnapManager -- set_layout & resize
    // ======================================================================

    #[test]
    fn set_layout_changes_preset() {
        let mut mgr = make_manager();
        mgr.set_layout(SnapLayoutPreset::SixGrid);
        assert_eq!(mgr.active_preset(), SnapLayoutPreset::SixGrid);
        assert_eq!(mgr.layout().zones.len(), 6);
    }

    #[test]
    fn setting_the_work_area_rebuilds_zones() {
        let mut mgr = make_manager();
        mgr.set_layout(SnapLayoutPreset::TwoEqualHalves);
        let old_width = mgr.layout().zones[0].width;

        let bigger = WorkArea::whole_screen(3840.0, 2160.0);
        mgr.set_work_area(bigger);
        let new_width = mgr.layout().zones[0].width;
        assert!(new_width > old_width);
        assert_eq!(mgr.work_area(), bigger);
    }

    #[test]
    fn moving_the_taskbar_moves_the_zones_rather_than_only_resizing_them() {
        // A resize that keeps the zones' *size* right and their *position*
        // wrong is precisely what the old screen-relative builder did, so
        // asserting on width alone (as the test above does, and as the whole
        // module used to) cannot see it. The bar moving from the bottom to the
        // top does not change the work area's size at all — only its origin.
        let mut mgr = SnapManager::new(DESK);
        let before = mgr.layout().zones[0].y;
        mgr.set_work_area(TOP_BAR_DESK);
        let after = mgr.layout().zones[0].y;
        assert!(
            (after - before - 48.0).abs() < 0.01,
            "zone top moved from {before} to {after}; expected a 48 px shift"
        );
    }

    // ======================================================================
    // SnapManager -- hit_test
    // ======================================================================

    #[test]
    fn hit_test_finds_left_zone() {
        let mgr = make_manager();
        let zone = mgr.hit_test(100.0, 540.0);
        assert!(zone.is_some());
        assert_eq!(zone.map(|z| z.id), Some(0));
    }

    #[test]
    fn hit_test_finds_right_zone() {
        let mgr = make_manager();
        let zone = mgr.hit_test(1800.0, 540.0);
        assert!(zone.is_some());
        assert_eq!(zone.map(|z| z.id), Some(1));
    }

    #[test]
    fn hit_test_returns_none_in_gap() {
        let mgr = make_manager();
        // The gap is right at the centre of 1920: (1920 - 6) / 2 = 957
        // so the gap is at x=957..963. Check the centre of the gap.
        let gap_x = (1920.0 - ZONE_GAP) / 2.0 + ZONE_GAP / 2.0;
        let result = mgr.hit_test(gap_x, 540.0);
        assert!(result.is_none(), "expected None in the gap area");
    }

    // ======================================================================
    // SnapManager -- choosing a tile
    // ======================================================================

    /// A chosen zone becomes a slot naming *that zone of the active layout*.
    /// Switching layout must change what the same zone number means, or the
    /// picker's whole purpose -- choosing among layouts -- is decorative.
    #[test]
    fn a_zone_names_a_slot_in_whichever_layout_is_active() {
        let mut mgr = make_manager();

        assert_eq!(
            mgr.slot_for_zone(1),
            SnapSlot::new(SnapLayoutPreset::TwoEqualHalves, 1)
        );

        mgr.set_layout(SnapLayoutPreset::SixGrid);
        assert_eq!(
            mgr.slot_for_zone(1),
            SnapSlot::new(SnapLayoutPreset::SixGrid, 1)
        );
        assert_ne!(
            mgr.slot_for_zone(1),
            SnapSlot::new(SnapLayoutPreset::TwoEqualHalves, 1),
            "zone 1 named the same tile in two different layouts"
        );
    }

    /// A zone the active layout does not have names nothing, rather than
    /// naming zone 0 or the last zone -- either of which would tile a window
    /// somewhere the user did not click.
    #[test]
    fn a_zone_the_layout_does_not_have_names_no_slot() {
        let mgr = make_manager();
        assert_eq!(
            mgr.slot_for_zone(2),
            None,
            "the two-half layout has 0 and 1"
        );
        assert_eq!(mgr.slot_for_zone(99), None);
        assert_eq!(mgr.slot_for_zone(ZoneId::MAX), None, "and does not wrap");
    }

    /// Every zone of every layout the picker offers can be named. A layout
    /// whose zones had no slots would be drawn, clickable, and inert.
    #[test]
    fn every_zone_of_every_offered_layout_can_be_asked_for() {
        let mut mgr = make_manager();
        for &preset in SnapLayoutPreset::all() {
            mgr.set_layout(preset);
            for zone in &mgr.layout().zones {
                assert!(
                    mgr.slot_for_zone(zone.id).is_some(),
                    "{preset:?} draws zone {} and cannot ask for it",
                    zone.id
                );
            }
        }
    }

    // ======================================================================
    // Rendering -- overlay
    // ======================================================================

    #[test]
    fn render_overlay_empty_when_hidden() {
        let mgr = make_manager();
        assert!(mgr.render_overlay(&accented(false)).is_empty());
    }

    #[test]
    fn render_overlay_nonempty_when_visible() {
        let mut mgr = make_manager();
        mgr.show_overlay();
        let cmds = mgr.render_overlay(&accented(false));
        // Scrim + (fill + stroke + text) * 2 zones = 7.
        assert!(cmds.len() >= 7);
    }

    #[test]
    fn render_zone_highlight_returns_commands() {
        let mgr = make_manager();
        let cmds = mgr.render_zone_highlight(&accented(false), 0);
        assert_eq!(cmds.len(), 3); // fill + stroke + text
    }

    #[test]
    fn render_zone_highlight_invalid_zone_empty() {
        let mgr = make_manager();
        assert!(mgr.render_zone_highlight(&accented(false), 99).is_empty());
    }

    // ======================================================================
    // Rendering -- picker
    // ======================================================================

    #[test]
    fn render_picker_empty_when_hidden() {
        let mgr = make_manager();
        assert!(mgr.render_picker(&accented(false)).is_empty());
    }

    #[test]
    fn render_picker_nonempty_when_visible() {
        let mut mgr = make_manager();
        mgr.show_picker();
        let cmds = mgr.render_picker(&accented(false));
        // At least shadow + bg + border + title + 7 thumbnails.
        assert!(cmds.len() >= 11);
    }

    // ======================================================================
    // Rendering -- colour
    //
    // Part 2 of the palette conversion (known-issues.md
    // `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`).
    // Every test below renders in *both* modes, because a conversion tested
    // only in the palette it was converted from cannot fail.
    // ======================================================================

    /// Every colour a command will put on the screen, in draw order.
    fn colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Text { color, .. }
                | RenderCommand::BoxShadow { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// How many mini-zones a preset's thumbnail draws.
    ///
    /// The *count* is reproduced from the production expression, the
    /// *colours* are not — so the pin table below can say what the picker
    /// draws without becoming a copy of the code that draws it.
    fn mini_zone_count(preset: SnapLayoutPreset) -> usize {
        preset
            .build(WorkArea::new(0.0, 0.0, THUMB_SIZE - 8.0, THUMB_SIZE - 8.0))
            .zones
            .len()
    }

    /// The fourteen named accents the appearance page offers.
    ///
    /// Shared by the two tests that must not be satisfied by a single sample.
    /// `accented()`'s magenta is the right fixture for asking "did this site
    /// follow the accent at all", because it is in no palette and so cannot be
    /// matched by accident. It is the wrong fixture for asking anything about
    /// a *function of* the accent: `readable_on` is a threshold, one accent
    /// samples one side of it, and the pair of hues in judgement 5 collides
    /// for exactly one of the fourteen. Walk the list for those.
    const OFFERED: [AccentColor; 14] = [
        AccentColor::Blue,
        AccentColor::Lavender,
        AccentColor::Teal,
        AccentColor::Green,
        AccentColor::Yellow,
        AccentColor::Peach,
        AccentColor::Pink,
        AccentColor::Mauve,
        AccentColor::Red,
        AccentColor::Rosewater,
        AccentColor::Flamingo,
        AccentColor::Maroon,
        AccentColor::Sky,
        AccentColor::Sapphire,
    ];

    /// `p` for `light` mode wearing `accent`, as the settings page would build.
    fn wearing(light: bool, accent: AccentColor) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = if light {
            accent.color_light()
        } else {
            accent.color()
        };
        p
    }

    /// A manager with the overlay up, the picker up and a zone hovered.
    fn everything_showing() -> SnapManager {
        let mut mgr = SnapManager::new(DESK);
        mgr.show_overlay();
        mgr.show_picker();
        mgr
    }

    #[test]
    fn every_colour_all_three_renderers_draw_comes_from_their_palette() {
        let mut drawn = 0;
        for light in [false, true] {
            let p = accented(light);
            for &preset in SnapLayoutPreset::all() {
                let mut mgr = everything_showing();
                mgr.set_layout(preset);
                for (i, _) in SnapLayoutPreset::all().iter().enumerate() {
                    let (ix, iy) = mgr.thumb_origin(i);
                    mgr.update_picker_hover(ix + THUMB_SIZE / 2.0, iy + THUMB_SIZE / 2.0);
                    let mut cmds = mgr.render_overlay(&p);
                    cmds.extend(mgr.render_picker(&p));
                    for zone in &mgr.layout.zones {
                        cmds.extend(mgr.render_zone_highlight(&p, zone.id));
                    }
                    drawn += colors(&cmds).len();
                    // One declared derivation: the lettering on a zone label,
                    // which is `readable_on` ink for the scrim it sits on
                    // (see the module docs). Everything else this module
                    // draws is a role, a role at an alpha, or black.
                    assert_drawn_from(&p, &cmds, &[readable_on(p.scrim())], "snap");
                }
            }
        }
        // The sweep is worthless if the fixtures drew nothing.
        assert!(drawn > 500, "only {drawn} colours were swept");
    }

    #[test]
    fn none_of_the_ten_deleted_constants_is_still_drawn() {
        // Rendered in *light* mode on purpose: every deleted constant was a
        // Mocha value, the light palette contains none of them, and so a
        // leftover names itself. Compared on RGB because three of the ten
        // differed only in alpha.
        const DELETED: [(&str, u32); 10] = [
            ("SURFACE0", 0x0031_3244),
            ("BLUE", 0x0089_B4FA),
            ("LAVENDER", 0x00B4_BEFE),
            ("TEXT", 0x00CD_D6F4),
            ("ZONE_FILL", 0x0089_B4FA),
            ("ZONE_HIGHLIGHT", 0x0089_B4FA),
            ("ZONE_BORDER", 0x0089_B4FA),
            ("OVERLAY_SCRIM", 0x001E_1E2E),
            ("PICKER_BG", 0x001E_1E2E),
            ("PICKER_HOVER", 0x0045_475A),
        ];
        let p = accented(true);
        let mut mgr = everything_showing();
        mgr.update_picker_hover(
            mgr.thumb_origin(0).0 + THUMB_SIZE / 2.0,
            mgr.thumb_origin(0).1 + THUMB_SIZE / 2.0,
        );
        let mut cmds = mgr.render_overlay(&p);
        cmds.extend(mgr.render_picker(&p));
        cmds.extend(mgr.render_zone_highlight(&p, 0));
        for c in colors(&cmds) {
            let rgb = (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
            for (name, hex) in DELETED {
                assert_ne!(
                    rgb, hex,
                    "the light render still draws Mocha {name}, so its constant \
                     survived the conversion"
                );
            }
        }
    }

    #[test]
    fn every_site_draws_the_role_it_claims() {
        // A membership sweep cannot see two sites *trading* colours — the set
        // drawn is identical either way (module 29's lesson). So compare the
        // ordered vector, per renderer, per mode.
        for light in [false, true] {
            let p = accented(light);
            let mode = if light { "light" } else { "dark" };

            let mut mgr = everything_showing();
            mgr.set_layout(SnapLayoutPreset::TwoEqualHalves);

            // --- overlay: scrim, then (fill, border, label) per zone ---
            let mut want = vec![p.scrim()];
            for _ in &mgr.layout.zones {
                want.extend([
                    p.selection_fill(),
                    p.selection_border(),
                    readable_on(p.scrim()),
                ]);
            }
            assert_eq!(
                colors(&mgr.render_overlay(&p)),
                want,
                "the {mode} overlay drew something other than what it claims"
            );

            // --- highlight: fill, border, label ---
            assert_eq!(
                colors(&mgr.render_zone_highlight(&p, 0)),
                vec![p.highlight_fill(), p.accent, readable_on(p.scrim())],
                "the {mode} zone highlight drew something other than what it claims"
            );

            // --- picker, with the third thumbnail hovered ---
            let hovered = 2;
            let (hx, hy) = mgr.thumb_origin(hovered);
            mgr.update_picker_hover(hx + THUMB_SIZE / 2.0, hy + THUMB_SIZE / 2.0);
            let mut want = vec![
                Color::rgba(0, 0, 0, 100),
                p.panel_bg(),
                p.surface0,
                p.lavender,
            ];
            for (i, &preset) in SnapLayoutPreset::all().iter().enumerate() {
                let active = mgr.active_preset == preset;
                if i == hovered {
                    want.push(p.panel_hover());
                }
                want.push(p.surface0);
                want.extend(std::iter::repeat_n(
                    if active { p.accent } else { p.overlay0 },
                    mini_zone_count(preset),
                ));
                if active {
                    want.push(p.accent);
                }
            }
            assert_eq!(
                colors(&mgr.render_picker(&p)),
                want,
                "the {mode} picker drew something other than what it claims"
            );
        }
    }

    #[test]
    fn a_zone_label_is_lettered_for_the_scrim_and_not_for_the_mode() {
        // The overlay lays a scrim over the work area before it draws a
        // label, and a scrim is black in both modes -- so the backdrop under
        // these labels does not change with the theme and neither may the
        // ink. `p.text` would be `#4C4F69` in light mode, on black.
        //
        // Pinned by hand rather than by calling `readable_on`: a test that
        // called the function the renderer calls would agree with it however
        // wrong both were (module 32's tautology lesson).
        //
        // Walked over all fourteen accents, not over the magenta fixture, and
        // that is what makes this a test rather than a coincidence. The wrong
        // implementation here is `readable_on(p.accent)` -- lettering the
        // label for the wash *over* the scrim instead of the scrim itself --
        // and `readable_on` answers with whichever endpoint is further from
        // what it is given. Half the accents share an answer with the scrim's,
        // so under those the wrong implementation returns the right value;
        // Yellow, Peach, Rosewater and Flamingo do not, and under those the
        // labels would go near-*black* on a black scrim. One accent is one
        // sample of a two-valued function, which is no sample at all.
        const NEAR_WHITE: u32 = 0x00EF_F1F5;
        let mut seen = Vec::new();
        for light in [false, true] {
            for accent in OFFERED {
                let p = wearing(light, accent);
                let mut mgr = everything_showing();
                mgr.set_layout(SnapLayoutPreset::TwoEqualHalves);

                let overlay = colors(&mgr.render_overlay(&p));
                let highlight = colors(&mgr.render_zone_highlight(&p, 0));
                // The zone label is the third of each zone's three commands;
                // the highlight's is its third and last.
                for (what, ink) in [
                    ("a resting zone", overlay[3]),
                    ("the hovered zone", highlight[2]),
                ] {
                    let rgb = (u32::from(ink.r) << 16) | (u32::from(ink.g) << 8) | u32::from(ink.b);
                    assert_eq!(
                        rgb,
                        NEAR_WHITE,
                        "{what}'s label is #{rgb:06X} in {} mode under {accent:?}, \
                         but it sits on a black scrim in every mode and under \
                         every accent",
                        if light { "light" } else { "dark" }
                    );
                    assert_ne!(
                        ink, p.text,
                        "{what}'s label took the mode's body ink, which is what \
                         loses it on the scrim under the light theme"
                    );
                    seen.push(rgb);
                }
            }
        }
        assert_eq!(
            seen,
            vec![NEAR_WHITE; 2 * OFFERED.len() * 2],
            "a label changed with the mode or with the accent"
        );
    }

    #[test]
    fn the_scrim_is_black_in_both_modes() {
        // §525 decision 3: a scrim is an absence of light, not a colour. The
        // constant this replaced was Mocha `base` at alpha 140, and Latte's
        // base is `#EFF1F5` -- so under the light theme it would have
        // *lightened* the desktop it exists to push back.
        for light in [false, true] {
            let p = accented(light);
            let mut mgr = SnapManager::new(DESK);
            mgr.show_overlay();
            assert_eq!(
                colors(&mgr.render_overlay(&p))[0],
                Color::rgba(0, 0, 0, 140),
                "the {} overlay's scrim is not black",
                if light { "light" } else { "dark" }
            );
        }
    }

    #[test]
    fn an_inactive_preset_is_never_the_accent_the_user_chose() {
        // Judgement 5, and the reason it is a test rather than a comment: the
        // thumbnails used to say "active" with blue and "inactive" with
        // lavender, and `AccentColor::Lavender` is one of the fourteen accents
        // the settings page offers. Under that accent the picker stopped
        // saying which layout was active at all.
        //
        // This walks all fourteen rather than one, because a pair of hues
        // collides for exactly one of them and a spot check picks the other
        // thirteen with probability 13/14.
        for light in [false, true] {
            for accent in OFFERED {
                let p = wearing(light, accent);
                let mut mgr = SnapManager::new(DESK);
                mgr.show_picker();
                mgr.set_layout(SnapLayoutPreset::TwoEqualHalves);
                let cols = colors(&mgr.render_picker(&p));
                assert!(
                    cols.contains(&p.accent),
                    "{accent:?}: the active preset is not marked at all"
                );
                assert!(
                    cols.contains(&p.overlay0),
                    "{accent:?}: no thumbnail is drawn as inactive"
                );
                assert_ne!(
                    p.accent,
                    p.overlay0,
                    "{accent:?} in {} mode collides with the rung the inactive \
                     thumbnails use, so the picker cannot say which layout is \
                     active",
                    if light { "light" } else { "dark" }
                );
            }
        }
    }

    #[test]
    fn the_picker_is_as_transparent_as_the_user_asked() {
        // The two constants this replaced froze an alpha into the literal --
        // `rgba(30, 30, 46, 230)` and `rgba(69, 71, 90, 200)` -- which is a
        // *setting* written down as a colour. `Palette::for_mode` pins
        // `panel_alpha` at 255, so at the default palette `p.base ==
        // p.panel_bg()` and every table-shaped test above agrees with that
        // bug. Only varying the setting sees it.
        let mut seen = Vec::new();
        for alpha in [255_u8, 200, 160] {
            let mut p = accented(false);
            p.panel_alpha = alpha;
            let mut mgr = SnapManager::new(DESK);
            mgr.show_overlay();
            mgr.show_picker();
            let (hx, hy) = mgr.thumb_origin(0);
            mgr.update_picker_hover(hx + THUMB_SIZE / 2.0, hy + THUMB_SIZE / 2.0);

            let picker = colors(&mgr.render_picker(&p));
            let bg = picker[1];
            assert_eq!((bg.r, bg.g, bg.b), (p.base.r, p.base.g, p.base.b));
            assert_eq!(bg.a, alpha, "the picker's background ignored the setting");
            let hover = picker[4];
            assert_eq!(
                (hover.r, hover.g, hover.b),
                (p.surface1.r, p.surface1.g, p.surface1.b)
            );
            assert_eq!(hover.a, alpha, "the picker's hover ignored the setting");

            // The two that must *not* follow it. A shadow and a scrim are
            // absences of light rather than panels; a picker that cast a
            // fainter shadow as it grew more transparent would be reporting
            // the setting twice, and a scrim that faded with it would stop
            // dimming the desktop the overlay is drawn over.
            assert_eq!(
                picker[0],
                Color::rgba(0, 0, 0, 100),
                "the shadow followed the setting"
            );
            assert_eq!(
                colors(&mgr.render_overlay(&p))[0],
                Color::rgba(0, 0, 0, 140),
                "the scrim followed the setting"
            );
            seen.push((bg.a, hover.a));
        }
        assert_eq!(seen, vec![(255, 255), (200, 200), (160, 160)]);
    }

    #[test]
    fn the_zone_under_the_cursor_out_reads_the_zones_at_rest() {
        // Both are the accent, so the only thing separating "one of eight
        // places this could go" from "the place it will go" is weight: a
        // louder wash and an opaque border against a washed one. A conversion
        // that gave both the same helper would compile, pass the membership
        // sweep, and leave the user unable to see where the window lands.
        for light in [false, true] {
            let p = accented(light);
            let mut mgr = SnapManager::new(DESK);
            mgr.show_overlay();
            let rest = colors(&mgr.render_overlay(&p));
            let hot = colors(&mgr.render_zone_highlight(&p, 0));
            assert!(
                hot[0].a > rest[1].a,
                "the hovered zone's fill is no louder than a resting one's"
            );
            assert!(
                hot[1].a > rest[2].a,
                "the hovered zone's border is no louder than a resting one's"
            );
            assert_eq!(
                (hot[0].r, hot[0].g, hot[0].b),
                (p.accent.r, p.accent.g, p.accent.b),
                "the hovered zone is not the accent"
            );
        }
    }

    // ======================================================================
    // Picker interaction
    // ======================================================================

    #[test]
    fn picker_trigger_near_top() {
        let mgr = make_manager();
        assert!(mgr.is_in_picker_trigger(960.0, 5.0));
        assert!(!mgr.is_in_picker_trigger(960.0, 200.0));
    }

    #[test]
    fn picker_select_changes_layout() {
        let mut mgr = make_manager();
        mgr.show_picker();
        // Manually set hover to the SixGrid preset (index 6).
        mgr.picker_hover_index = Some(6);
        assert!(mgr.picker_select());
        assert_eq!(mgr.active_preset(), SnapLayoutPreset::SixGrid);
        assert!(!mgr.is_picker_visible());
    }

    #[test]
    fn hovering_each_thumbnail_selects_that_preset() {
        // Hit-testing and drawing used to compute the picker grid from
        // separate copies of the same expression, four hundred lines apart.
        // They now share `thumb_origin`, and this walks every thumbnail's
        // centre to confirm the grid the user points at is the grid they get
        // — including for a picker that is not at the screen origin.
        let mut mgr = SnapManager::new(TOP_BAR_DESK);
        mgr.show_picker();
        for (i, &preset) in SnapLayoutPreset::all().iter().enumerate() {
            let (ix, iy) = mgr.thumb_origin(i);
            mgr.update_picker_hover(ix + THUMB_SIZE / 2.0, iy + THUMB_SIZE / 2.0);
            assert_eq!(
                mgr.picker_hover_index,
                Some(i),
                "the centre of thumbnail {i} ({preset:?}) did not hover it"
            );
        }
    }

    #[test]
    fn the_gap_between_thumbnails_hovers_nothing() {
        // The complement of the test above: a grid whose cells are too large
        // would pass it and still hover the wrong preset near a boundary.
        let mut mgr = SnapManager::new(TOP_BAR_DESK);
        mgr.show_picker();
        let (ix, iy) = mgr.thumb_origin(0);
        mgr.update_picker_hover(ix + THUMB_SIZE + THUMB_GAP / 2.0, iy + THUMB_SIZE / 2.0);
        assert_eq!(mgr.picker_hover_index, None);
    }

    #[test]
    fn picker_select_without_hover_returns_false() {
        let mut mgr = make_manager();
        mgr.show_picker();
        assert!(!mgr.picker_select());
    }

    // ======================================================================
    // Zone-by-id lookup
    // ======================================================================

    #[test]
    fn zone_by_id_found() {
        let mgr = make_manager();
        let z = mgr.zone_by_id(1);
        assert!(z.is_some());
        assert_eq!(z.map(|zz| zz.label), Some("Right"));
    }

    #[test]
    fn zone_by_id_not_found() {
        let mgr = make_manager();
        assert!(mgr.zone_by_id(99).is_none());
    }

    // ======================================================================
    // Preset labels are non-empty
    // ======================================================================

    #[test]
    fn all_preset_labels_nonempty() {
        for preset in SnapLayoutPreset::all() {
            assert!(!preset.label().is_empty(), "{preset:?} has empty label");
        }
    }
}
