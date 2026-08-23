//! Mouse and keyboard settings panel: the rendering, not the model.
//!
//! The types this panel edits — [`MouseConfig`], [`KeyboardRepeatConfig`] and
//! the enums around them — live in the `inputsettings` crate and are
//! re-exported here, because the compositor and the Settings application need
//! exactly the same values and none of the three can depend on the other two.
//! Until 2026-08-22 they lived *here*, with no persistence at all: a user
//! could drag the double-click slider, watch the number change, and find that
//! nothing behaved differently and the value was gone at the next login. See
//! `known-issues.md` `TD-C-THE-MOUSE-SETTINGS-PANEL-REACHES-NOTHING`.
//!
//! What stays here is [`InputSettingsUI`]: the expand/collapse state, the
//! dirty flag, the render commands and the hit-testing.
//!
//! # Colour
//!
//! Every colour is read from the [`Palette`] the caller supplies; this module
//! holds none of its own. Four judgements decide which role goes where, and
//! each of them is a test rather than a comment.
//!
//! 1. **The accent says what is in force, and says nothing else.** Exactly two
//!    things are drawn in it: the heading of the section that is open, and the
//!    filled part of a slider track. Both answer the same question — *this is
//!    the one you are working on, and this much of it is set*. The count is
//!    asserted, so a third accented thing is a failure rather than a taste
//!    disagreement.
//! 2. **A state is not a position.** The on-pill is [`Palette::green`] and the
//!    unsaved-changes banner is [`Palette::yellow`], and neither follows the
//!    accent — because a switch can be *on* inside a section that is *closed*,
//!    and unsaved changes belong to the whole panel rather than to whichever
//!    part of it the user happens to be looking at. Painting them with the
//!    accent would make "in force" and "switched on" the same colour, which
//!    are different questions with different answers on the same screen.
//! 3. **The slider thumb is derived from its fill, never named beside it.** It
//!    is [`emphasized`] of the accent. Until this conversion the thumb was
//!    `LAVENDER` and the fill was `BLUE`: two constants that looked related
//!    only because Catppuccin put them near each other, and were free to drift
//!    apart the moment either was touched. A derivation cannot drift.
//! 4. **Three sites choose their colour by state, so the tests render both
//!    branches.** The section header's background, its heading ink, and the
//!    toggle pill each pick between two roles. A branch the fixture never
//!    renders is a branch the tests never check.

use appearance::{Palette, emphasized};
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

// ============================================================================
// The model, which lives elsewhere
// ============================================================================

// Re-exported rather than merely `use`d so that every existing caller of
// `desktop::mouse_settings::MouseConfig` keeps working, and so that the panel
// and the model still read as one thing from the outside.
pub use inputsettings::{
    AccelProfile, ButtonMapping, InputFile, InputSettings, KeyboardRepeatConfig, MouseConfig,
    ScrollMode,
};

// ============================================================================
// Settings panel rendering
// ============================================================================

/// Render state for the mouse/keyboard settings panel.
pub struct InputSettingsUI {
    settings: InputSettings,
    /// Which section is expanded: 0 = mouse pointer, 1 = buttons, 2 = scroll,
    /// 3 = cursor, 4 = keyboard.
    expanded_section: usize,
    /// Dirty flag indicating unsaved changes.
    dirty: bool,
    /// Last saved snapshot (for revert).
    saved: InputSettings,
}

impl InputSettingsUI {
    pub fn new() -> Self {
        let settings = InputSettings::default();
        let saved = settings.clone();
        Self {
            settings,
            expanded_section: 0,
            dirty: false,
            saved,
        }
    }

    pub fn with_settings(settings: InputSettings) -> Self {
        let saved = settings.clone();
        Self {
            settings,
            expanded_section: 0,
            dirty: false,
            saved,
        }
    }

    pub fn settings(&self) -> &InputSettings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut InputSettings {
        self.dirty = true;
        &mut self.settings
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_saved(&mut self) {
        self.dirty = false;
        self.saved = self.settings.clone();
    }

    pub fn revert(&mut self) {
        self.settings = self.saved.clone();
        self.dirty = false;
    }

    pub fn expand_section(&mut self, section: usize) {
        if section <= 4 {
            self.expanded_section = section;
        }
    }

    pub fn expanded_section(&self) -> usize {
        self.expanded_section
    }

    // ------------------------------------------------------------------
    // Section headers
    // ------------------------------------------------------------------

    const SECTIONS: [&'static str; 5] = [
        "Pointer Speed & Acceleration",
        "Buttons",
        "Scrolling",
        "Cursor Appearance",
        "Keyboard Repeat",
    ];

    // ------------------------------------------------------------------
    // Rendering
    // ------------------------------------------------------------------

    /// Render the settings panel into a list of render commands.
    pub fn render(&self, p: &Palette, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let mut cy = y;
        let pad = 16.0_f32;
        let inner = width - 2.0 * pad;

        // Panel background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: 900.0,
            color: p.base,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cy += pad;
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: cy,
            text: "Mouse & Keyboard Settings".into(),
            font_size: 20.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(inner),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 32.0;

        // Sections
        for (i, section_name) in Self::SECTIONS.iter().enumerate() {
            let expanded = self.expanded_section == i;
            let indicator = if expanded { "▼" } else { "▶" };

            // Section header
            cmds.push(RenderCommand::FillRect {
                x: x + pad,
                y: cy,
                width: inner,
                height: 36.0,
                // Two rungs either side of the panel it sits on: `surface0` is
                // in front of `base`, `mantle` is behind it. So the section
                // being edited stands proud of the panel and the four that are
                // not recede into it, in both modes — a claim about the
                // palette's stacking order rather than about brightness, which
                // reverses between Mocha and Latte.
                color: if expanded { p.surface0 } else { p.mantle },
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + pad + 12.0,
                y: cy + 10.0,
                text: format!("{indicator} {section_name}"),
                font_size: 14.0,
                // Judgement 1: the open section is the one in force.
                color: if expanded { p.accent } else { p.text },
                font_weight: FontWeightHint::Bold,
                max_width: Some(inner - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 40.0;

            if expanded {
                match i {
                    0 => cy = self.render_pointer_section(&mut cmds, p, x + pad, cy, inner),
                    1 => cy = self.render_buttons_section(&mut cmds, p, x + pad, cy, inner),
                    2 => cy = self.render_scroll_section(&mut cmds, p, x + pad, cy, inner),
                    3 => cy = self.render_cursor_section(&mut cmds, p, x + pad, cy, inner),
                    4 => cy = self.render_keyboard_section(&mut cmds, p, x + pad, cy, inner),
                    _ => {}
                }
                cy += 8.0;
            }
        }

        // Dirty indicator / action bar
        cy += 8.0;
        if self.dirty {
            cmds.push(RenderCommand::FillRect {
                x: x + pad,
                y: cy,
                width: inner,
                height: 36.0,
                color: p.surface0,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + pad + 12.0,
                y: cy + 10.0,
                text: "Unsaved changes — press Apply to save or Revert to discard".into(),
                font_size: 13.0,
                // Judgement 2: a named warning hue, not the accent. Unsaved
                // changes are a fact about the whole panel, and stay true while
                // the user is looking at some other section entirely.
                color: p.yellow,
                font_weight: FontWeightHint::Regular,
                max_width: Some(inner - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        cmds
    }

    // ------------------------------------------------------------------
    // Per-section renderers
    // ------------------------------------------------------------------

    fn render_pointer_section(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        mut y: f32,
        width: f32,
    ) -> f32 {
        let m = &self.settings.mouse;

        y = self.render_label_value(cmds, p, x, y, width, "Speed", &format!("{}", m.speed));
        y = self.render_slider_hint(cmds, p, x, y, width, -10, 10, m.speed);

        y = self.render_label_value(
            cmds,
            p,
            x,
            y,
            width,
            "Acceleration profile",
            m.accel_profile.label(),
        );

        if m.accel_profile == AccelProfile::Custom {
            y = self.render_label_value(
                cmds,
                p,
                x,
                y,
                width,
                "Accel gain",
                &format!("{:.1}", m.accel_gain),
            );
            y = self.render_label_value(
                cmds,
                p,
                x,
                y,
                width,
                "Accel threshold",
                &format!("{}", m.accel_threshold),
            );
        }

        y
    }

    fn render_buttons_section(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        mut y: f32,
        width: f32,
    ) -> f32 {
        let m = &self.settings.mouse;

        y = self.render_label_value(
            cmds,
            p,
            x,
            y,
            width,
            "Button layout",
            m.button_mapping.label(),
        );
        y = self.render_label_value(
            cmds,
            p,
            x,
            y,
            width,
            "Double-click speed",
            &format!("{} ms", m.double_click_ms),
        );
        y = self.render_slider_hint(cmds, p, x, y, width, 100, 2000, m.double_click_ms as i32);

        y
    }

    fn render_scroll_section(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        mut y: f32,
        width: f32,
    ) -> f32 {
        let m = &self.settings.mouse;

        y = self.render_label_value(cmds, p, x, y, width, "Scroll mode", m.scroll_mode.label());

        if m.scroll_mode == ScrollMode::Lines {
            y = self.render_label_value(
                cmds,
                p,
                x,
                y,
                width,
                "Lines per notch",
                &format!("{}", m.scroll_lines),
            );
        }

        if m.scroll_mode == ScrollMode::Smooth {
            y = self.render_label_value(
                cmds,
                p,
                x,
                y,
                width,
                "Scroll speed",
                &format!("{:.1}×", m.scroll_speed),
            );
        }

        y = self.render_toggle(cmds, p, x, y, width, "Natural scroll", m.natural_scroll);

        y
    }

    fn render_cursor_section(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        mut y: f32,
        width: f32,
    ) -> f32 {
        let m = &self.settings.mouse;

        y = self.render_label_value(
            cmds,
            p,
            x,
            y,
            width,
            "Cursor size",
            &format!("{} px", m.cursor_size),
        );
        y = self.render_toggle(
            cmds,
            p,
            x,
            y,
            width,
            "Locate on Ctrl press",
            m.locate_on_ctrl,
        );
        y = self.render_toggle(
            cmds,
            p,
            x,
            y,
            width,
            "Hide while typing",
            m.hide_while_typing,
        );
        y = self.render_toggle(cmds, p, x, y, width, "Show cursor trail", m.show_trail);

        if m.show_trail {
            y = self.render_label_value(
                cmds,
                p,
                x,
                y,
                width,
                "Trail length",
                &format!("{}", m.trail_length),
            );
        }

        y
    }

    fn render_keyboard_section(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        mut y: f32,
        width: f32,
    ) -> f32 {
        let k = &self.settings.keyboard;

        y = self.render_toggle(cmds, p, x, y, width, "Key repeat enabled", k.enabled);

        if k.enabled {
            y = self.render_label_value(
                cmds,
                p,
                x,
                y,
                width,
                "Repeat delay",
                &format!("{} ms", k.repeat_delay_ms),
            );
            y = self.render_slider_hint(cmds, p, x, y, width, 150, 2000, k.repeat_delay_ms as i32);

            y = self.render_label_value(
                cmds,
                p,
                x,
                y,
                width,
                "Repeat interval",
                &format!("{} ms", k.repeat_interval_ms),
            );
            y = self.render_slider_hint(cmds, p, x, y, width, 10, 500, k.repeat_interval_ms as i32);
        }

        y
    }

    // ------------------------------------------------------------------
    // Shared rendering helpers
    // ------------------------------------------------------------------

    fn render_label_value(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        value: &str,
    ) -> f32 {
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y,
            text: label.into(),
            font_size: 13.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.55,
            y,
            text: value.into(),
            font_size: 13.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.4),
            overflow: TextOverflow::Ellipsis,
        });
        y + 22.0
    }

    fn render_toggle(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        on: bool,
    ) -> f32 {
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y,
            text: label.into(),
            font_size: 13.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.6),
            overflow: TextOverflow::Ellipsis,
        });

        // Toggle pill
        let tx = x + width - 48.0;
        // Judgement 2: `green` because it reports a state, not `accent`
        // because it does not report a position. A switch stays on inside a
        // section the user has since collapsed.
        let bg = if on { p.green } else { p.surface1 };
        cmds.push(RenderCommand::FillRect {
            x: tx,
            y,
            width: 40.0,
            height: 20.0,
            color: bg,
            corner_radii: CornerRadii::all(10.0),
        });
        let knob_x = if on { tx + 22.0 } else { tx + 2.0 };
        cmds.push(RenderCommand::FillRect {
            x: knob_x,
            y: y + 2.0,
            width: 16.0,
            height: 16.0,
            color: p.text,
            corner_radii: CornerRadii::all(8.0),
        });

        y + 26.0
    }

    fn render_slider_hint(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
        min: i32,
        max: i32,
        value: i32,
    ) -> f32 {
        let track_x = x + 8.0;
        let track_w = width - 16.0;
        let track_h = 6.0_f32;

        // Track background
        cmds.push(RenderCommand::FillRect {
            x: track_x,
            y: y + 4.0,
            width: track_w,
            height: track_h,
            color: p.surface1,
            corner_radii: CornerRadii::all(3.0),
        });

        // Fill
        // A track whose `max` is below its `min` is a caller error, not a
        // reason to underflow: it yields an empty range and a fill of zero.
        let range = max.saturating_sub(min).max(1) as f32;
        let frac = (value.saturating_sub(min) as f32 / range).clamp(0.0, 1.0);
        cmds.push(RenderCommand::FillRect {
            x: track_x,
            y: y + 4.0,
            width: track_w * frac,
            height: track_h,
            // Judgement 1: how much of this control is set.
            color: p.accent,
            corner_radii: CornerRadii::all(3.0),
        });

        // Thumb
        let thumb_x = track_x + track_w * frac - 7.0;
        cmds.push(RenderCommand::FillRect {
            x: thumb_x,
            y,
            width: 14.0,
            height: 14.0,
            // Judgement 3: derived from the fill it sits at the end of, so the
            // handle and the track it handles cannot come out unrelated
            // colours. It was `LAVENDER` beside a `BLUE` fill, which looked
            // deliberate only because Catppuccin lists them near each other.
            color: emphasized(p.accent),
            corner_radii: CornerRadii::all(7.0),
        });

        y + 20.0
    }

    // ------------------------------------------------------------------
    // Hit-testing
    // ------------------------------------------------------------------

    /// Returns the index of the section header hit, or `None`.
    pub fn hit_section(&self, rel_y: f32) -> Option<usize> {
        // Each section header occupies 36px with 4px gap; title area is ~48px.
        let after_title = rel_y - 48.0;
        if after_title < 0.0 {
            return None;
        }
        // Walk through sections, accounting for expanded content.
        let mut oy = 0.0_f32;
        for i in 0..5 {
            if after_title >= oy && after_title < oy + 36.0 {
                return Some(i);
            }
            oy += 40.0;
            if self.expanded_section == i {
                // Approximate content heights per section.
                oy += match i {
                    0 => 90.0,
                    1 => 70.0,
                    2 => 90.0,
                    3 => 100.0,
                    4 => 80.0,
                    _ => 0.0,
                };
            }
        }
        None
    }
}

impl Default for InputSettingsUI {
    fn default() -> Self {
        Self::new()
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
    // These tests select a rectangle by the exact literal dimensions the code
    // under test was handed — 40 by 20 is *the* toggle pill, and nothing else.
    // That is the assertion meant: a tolerance would let a rectangle that has
    // been resized pass as one that has not, and geometry is the only handle
    // this panel offers on which of its unnamed boxes is which.
    #![allow(clippy::float_cmp)]

    use super::*;
    use crate::palette_check::assert_drawn_from;
    use guitk::color::Color;

    /// A palette whose accent belongs to no palette.
    ///
    /// The stock accent is `blue`, and judgement 1 is a claim about *how many*
    /// things this panel accents. At the shipped theme that claim is
    /// unfalsifiable in both directions: a site left drawing `blue` by mistake
    /// would be counted as accented, and the real accent could not be told
    /// apart from it. The loop proves the substitute is genuinely outside the
    /// palette rather than coincidentally equal to a role a test then reads by
    /// accident.
    fn accented(light: bool) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = Color::from_hex(0xFF00FF);
        for (name, role) in p.roles() {
            if name == "accent" {
                continue;
            }
            assert_ne!(
                (role.r, role.g, role.b),
                (p.accent.r, p.accent.g, p.accent.b),
                "the substitute accent collides with {name}, so an accent                  assertion would be reading that role instead"
            );
        }
        p
    }

    #[test]
    fn ui_dirty_tracking() {
        let mut ui = InputSettingsUI::new();
        assert!(!ui.is_dirty());
        ui.settings_mut().mouse.set_speed(3);
        assert!(ui.is_dirty());
        ui.mark_saved();
        assert!(!ui.is_dirty());
    }

    #[test]
    fn ui_revert() {
        let mut ui = InputSettingsUI::new();
        ui.settings_mut().mouse.set_speed(7);
        assert!(ui.is_dirty());
        ui.revert();
        assert!(!ui.is_dirty());
        assert_eq!(ui.settings().mouse.speed, 0);
    }

    #[test]
    fn ui_expand_section() {
        let mut ui = InputSettingsUI::new();
        assert_eq!(ui.expanded_section(), 0);
        ui.expand_section(3);
        assert_eq!(ui.expanded_section(), 3);
        ui.expand_section(99);
        assert_eq!(ui.expanded_section(), 3); // out of range ignored
    }

    #[test]
    fn ui_render_produces_commands() {
        let ui = InputSettingsUI::new();
        let cmds = ui.render(&accented(false), 0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn ui_render_with_dirty() {
        let p = accented(false);
        let mut ui = InputSettingsUI::new();
        ui.settings_mut().mouse.set_speed(3);
        let cmds = ui.render(&p, 0.0, 0.0, 400.0);
        // Should contain a yellow "unsaved changes" text.
        let has_yellow = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { color, .. } if *color == p.yellow));
        assert!(has_yellow);
    }

    #[test]
    fn ui_render_each_section() {
        let mut ui = InputSettingsUI::new();
        for i in 0..5 {
            ui.expand_section(i);
            let cmds = ui.render(&accented(false), 0.0, 0.0, 400.0);
            assert!(!cmds.is_empty());
        }
    }

    #[test]
    fn ui_custom_accel_shows_extra_fields() {
        let mut ui = InputSettingsUI::new();
        ui.settings_mut().mouse.accel_profile = AccelProfile::Custom;
        ui.expand_section(0);
        let cmds = ui.render(&accented(false), 0.0, 0.0, 400.0);
        let has_gain = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Accel gain")));
        assert!(has_gain);
    }

    #[test]
    fn ui_hit_section_before_title() {
        let ui = InputSettingsUI::new();
        assert!(ui.hit_section(10.0).is_none());
    }

    #[test]
    fn ui_hit_section_first() {
        let ui = InputSettingsUI::new();
        // First header starts at about y=48.
        let hit = ui.hit_section(50.0);
        assert_eq!(hit, Some(0));
    }

    #[test]
    fn show_trail_toggle_renders_trail_length() {
        let mut ui = InputSettingsUI::new();
        ui.settings_mut().mouse.show_trail = true;
        ui.expand_section(3);
        let cmds = ui.render(&accented(false), 0.0, 0.0, 400.0);
        let has_trail = cmds.iter().any(
            |c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Trail length")),
        );
        assert!(has_trail);
    }

    #[test]
    fn keyboard_disabled_hides_sliders() {
        let mut ui = InputSettingsUI::new();
        ui.settings_mut().keyboard.enabled = false;
        ui.expand_section(4);
        let cmds = ui.render(&accented(false), 0.0, 0.0, 400.0);
        let has_delay = cmds.iter().any(
            |c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Repeat delay")),
        );
        assert!(!has_delay);
    }

    #[test]
    fn with_settings_constructor() {
        let mut s = InputSettings::new();
        s.mouse.set_speed(5);
        let ui = InputSettingsUI::with_settings(s);
        assert_eq!(ui.settings().mouse.speed, 5);
        assert!(!ui.is_dirty());
    }

    #[test]
    fn smooth_scroll_shows_speed() {
        let mut ui = InputSettingsUI::new();
        ui.settings_mut().mouse.scroll_mode = ScrollMode::Smooth;
        ui.expand_section(2);
        let cmds = ui.render(&accented(false), 0.0, 0.0, 400.0);
        let has_speed = cmds.iter().any(
            |c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Scroll speed")),
        );
        assert!(has_speed);
    }

    #[test]
    fn lines_scroll_shows_lines_per_notch() {
        let mut ui = InputSettingsUI::new();
        ui.settings_mut().mouse.scroll_mode = ScrollMode::Lines;
        ui.expand_section(2);
        let cmds = ui.render(&accented(false), 0.0, 0.0, 400.0);
        let has_lines = cmds.iter().any(
            |c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Lines per notch")),
        );
        assert!(has_lines);
    }

    // ======================================================================
    // Colour — TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE
    // ======================================================================

    /// The panel width every colour test renders at, and the inner width that
    /// follows from it — a section header is `width - 2 * pad` wide, and that
    /// is how the tests below tell a header apart from every other rectangle.
    const PANEL_W: f32 = 400.0;
    const INNER_W: f32 = 368.0;

    /// The panel with `section` open, every switch at `on`, and the unsaved
    /// banner showing when `dirty`.
    ///
    /// `on` is a parameter rather than a default because the toggle pill is one
    /// of three sites that choose their colour by state, and a fixture that
    /// only ever renders switched-off pills leaves `green` untested — which is
    /// judgement 4's whole point. `speed` is moved off its default so that the
    /// slider draws a fill narrower than its track: at the default the two
    /// rectangles are the same size and neither test nor eye could tell which
    /// was which.
    fn panel(section: usize, on: bool, dirty: bool) -> InputSettingsUI {
        let mut ui = InputSettingsUI::new();
        {
            let s = ui.settings_mut();
            s.mouse.set_speed(7);
            s.mouse.natural_scroll = on;
            s.mouse.locate_on_ctrl = on;
            s.mouse.hide_while_typing = on;
            s.mouse.show_trail = on;
            s.keyboard.enabled = on;
        }
        ui.expand_section(section);
        if !dirty {
            ui.mark_saved();
        }
        ui
    }

    /// The colour a command puts on the screen, if it puts one there at all.
    fn color_of(cmd: &RenderCommand) -> Option<Color> {
        match cmd {
            RenderCommand::FillRect { color, .. }
            | RenderCommand::StrokeRect { color, .. }
            | RenderCommand::Text { color, .. }
            | RenderCommand::Line { color, .. }
            | RenderCommand::BoxShadow { color, .. } => Some(*color),
            _ => None,
        }
    }

    /// The colour of the one `Text` command reading `s`.
    fn text_color(cmds: &[RenderCommand], s: &str) -> Color {
        let found: Vec<Color> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, color, .. } if text == s => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(found.len(), 1, "expected exactly one command reading {s:?}");
        found[0]
    }

    /// The colours of every `FillRect` of exactly `w` by `h` points, in draw
    /// order. Geometry is the only handle this panel offers on which rectangle
    /// is which: nothing it draws carries a name.
    fn fills_sized(cmds: &[RenderCommand], w: f32, h: f32) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    width,
                    height,
                    color,
                    ..
                } if *width == w && *height == h => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Width and colour of every `FillRect` exactly `h` points tall — the
    /// slider track and the part of it that is set, which differ in width by
    /// construction and so cannot be selected by size alone.
    fn fills_high(cmds: &[RenderCommand], h: f32) -> Vec<(f32, Color)> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    width,
                    height,
                    color,
                    ..
                } if *height == h => Some((*width, *color)),
                _ => None,
            })
            .collect()
    }

    /// Nothing this panel draws comes from outside the palette it was given.
    ///
    /// Every configuration that changes *which* commands are produced: five
    /// sections, switches both ways, banner both ways. A constant left behind
    /// in a branch the fixture never renders is a constant the sweep cannot
    /// see, and this module has three sites that pick a colour by state.
    #[test]
    fn every_colour_this_panel_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = accented(light);
            // The thumb is the one colour here that is in no palette at all;
            // judgement 3 is precisely that it is computed from the fill.
            let derived = [emphasized(p.accent)];
            for section in 0..5 {
                for on in [false, true] {
                    for dirty in [false, true] {
                        let cmds = panel(section, on, dirty).render(&p, 0.0, 0.0, PANEL_W);
                        assert!(
                            cmds.len() > 12,
                            "section {section} drew {} commands, which is not \
                             a settings panel",
                            cmds.len()
                        );
                        assert_drawn_from(&p, &cmds, &derived, "input settings");
                    }
                }
            }
        }
    }

    /// None of the ten deleted constants is still drawn.
    ///
    /// Every value below is a Catppuccin **Mocha** colour, so a light render
    /// cannot legitimately produce one — which turns "a substitution was
    /// missed" from an invisible defect into a named failure.
    #[test]
    fn none_of_the_ten_deleted_constants_is_still_drawn() {
        const DELETED: [(&str, u32); 10] = [
            ("BASE", 0x001E_1E2E),
            ("MANTLE", 0x0018_1825),
            ("SURFACE0", 0x0031_3244),
            ("SURFACE1", 0x0045_475A),
            ("TEXT", 0x00CD_D6F4),
            ("SUBTEXT0", 0x00A6_ADC8),
            ("BLUE", 0x0089_B4FA),
            ("GREEN", 0x00A6_E3A1),
            ("YELLOW", 0x00F9_E2AF),
            ("LAVENDER", 0x00B4_BEFE),
        ];

        let p = accented(true);
        let mut cmds = Vec::new();
        for section in 0..5 {
            for on in [false, true] {
                cmds.extend(panel(section, on, true).render(&p, 0.0, 0.0, PANEL_W));
            }
        }
        for c in cmds.iter().filter_map(color_of) {
            let rgb = (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
            for (name, deleted) in DELETED {
                assert_ne!(
                    rgb, deleted,
                    "the panel still draws the deleted constant {name} \
                     (#{deleted:06X}) in a light render"
                );
            }
        }
    }

    /// Every site, named one at a time, in the role this module claims for it.
    ///
    /// The sweep above proves only *membership*, and membership cannot see a
    /// swap: a header painted `surface1` instead of `surface0` draws a legal
    /// colour, and so does a value painted `subtext0` instead of `text`.
    /// Fourteen source sites need fourteen assertions, so this is that table.
    #[test]
    fn every_site_draws_the_role_it_claims() {
        for light in [false, true] {
            let p = accented(light);

            let cmds = panel(0, false, false).render(&p, 0.0, 0.0, PANEL_W);

            // The panel and its title.
            assert_eq!(
                cmds.first().and_then(color_of),
                Some(p.base),
                "the panel does not start with its own background"
            );
            assert_eq!(
                text_color(&cmds, "Mouse & Keyboard Settings"),
                p.text,
                "the panel title is the brightest thing on it"
            );

            // The five section headers, the open one first.
            assert_eq!(
                fills_sized(&cmds, INNER_W, 36.0),
                vec![p.surface0, p.mantle, p.mantle, p.mantle, p.mantle],
                "the section headers"
            );
            assert_eq!(
                text_color(&cmds, "▼ Pointer Speed & Acceleration"),
                p.accent,
                "the open section's heading"
            );
            assert_eq!(
                text_color(&cmds, "▶ Buttons"),
                p.text,
                "a closed section's heading"
            );

            // A setting: its name is dimmer than its value, because the value
            // is what the user came to read.
            assert_eq!(text_color(&cmds, "Speed"), p.subtext0, "a setting's label");
            assert_eq!(text_color(&cmds, "7"), p.text, "a setting's value");

            // The slider: a track, the part of it that is set, and the handle.
            let bars = fills_high(&cmds, 6.0);
            assert_eq!(bars.len(), 2, "one slider draws a track and a fill");
            assert_eq!(bars[0].1, p.surface1, "the slider track");
            assert_eq!(bars[1].1, p.accent, "the part of the track that is set");
            assert!(
                bars[1].0 < bars[0].0,
                "the fill covers the whole track, so nothing can tell the two \
                 apart and the pinning above is checking one rectangle twice"
            );
            assert_eq!(
                fills_sized(&cmds, 14.0, 14.0),
                vec![emphasized(p.accent)],
                "the slider thumb"
            );

            // The toggle, both ways round.
            for on in [false, true] {
                let cmds = panel(2, on, false).render(&p, 0.0, 0.0, PANEL_W);
                assert_eq!(
                    text_color(&cmds, "Natural scroll"),
                    p.subtext0,
                    "a toggle's label"
                );
                assert_eq!(
                    fills_sized(&cmds, 40.0, 20.0),
                    vec![if on { p.green } else { p.surface1 }],
                    "the pill of a switch that is {on}"
                );
                assert_eq!(
                    fills_sized(&cmds, 16.0, 16.0),
                    vec![p.text],
                    "the knob of a switch that is {on}"
                );
            }

            // The unsaved-changes banner, which is the sixth box of the header
            // size and is drawn last.
            let cmds = panel(2, false, true).render(&p, 0.0, 0.0, PANEL_W);
            let boxes = fills_sized(&cmds, INNER_W, 36.0);
            assert_eq!(boxes.len(), 6, "five section headers and the banner");
            assert_eq!(boxes[5], p.surface0, "the unsaved-changes banner");
            assert_eq!(
                text_color(
                    &cmds,
                    "Unsaved changes — press Apply to save or Revert to discard"
                ),
                p.yellow,
                "the unsaved-changes text"
            );
        }
    }

    /// Judgement 1: the accent says what is in force, and says nothing else.
    ///
    /// Counted rather than listed, because a set-membership check cannot see a
    /// *third* accented thing appearing — and "the accent is used tastefully"
    /// is not a property any test can read off a command list. The counts are
    /// written out by hand: deriving them from the same `if` the renderer uses
    /// would make this test agree with whatever the renderer does.
    ///
    /// The banner is rendered both ways at every row, so an unsaved-changes
    /// warning painted in the accent — which is exactly the mistake judgement
    /// 2 exists to forbid — pushes the count off by one.
    #[test]
    fn only_what_is_in_force_is_accented() {
        // (open section, switches on, how many accented commands).
        // One heading, plus one track fill per slider on show: pointer has the
        // speed slider, buttons the double-click slider, scrolling and cursor
        // none, and the keyboard's two only exist while key repeat is enabled.
        const ACCENTED: [(usize, bool, usize); 10] = [
            (0, false, 2),
            (0, true, 2),
            (1, false, 2),
            (1, true, 2),
            (2, false, 1),
            (2, true, 1),
            (3, false, 1),
            (3, true, 1),
            (4, false, 1),
            (4, true, 3),
        ];

        for light in [false, true] {
            let p = accented(light);
            for (section, on, expected) in ACCENTED {
                for dirty in [false, true] {
                    let cmds = panel(section, on, dirty).render(&p, 0.0, 0.0, PANEL_W);
                    let hits: Vec<&RenderCommand> = cmds
                        .iter()
                        .filter(|c| color_of(c) == Some(p.accent))
                        .collect();
                    assert_eq!(
                        hits.len(),
                        expected,
                        "section {section} with switches {on} and dirty \
                         {dirty} accents {} things, not {expected}",
                        hits.len()
                    );

                    // And they are the right things: one heading, the rest
                    // track fills. A count alone would pass a panel that
                    // accented the banner and left a heading plain.
                    let headings: Vec<&String> = hits
                        .iter()
                        .filter_map(|c| match c {
                            RenderCommand::Text { text, .. } => Some(text),
                            _ => None,
                        })
                        .collect();
                    assert_eq!(
                        headings.len(),
                        1,
                        "exactly one piece of text is in force at a time"
                    );
                    assert!(
                        headings[0].starts_with('▼'),
                        "the accented heading is {:?}, which is not the open \
                         section",
                        headings[0]
                    );
                    for c in &hits {
                        match c {
                            RenderCommand::Text { .. } => {}
                            RenderCommand::FillRect { height, .. } => assert_eq!(
                                *height, 6.0,
                                "an accented rectangle that is not a slider fill"
                            ),
                            other => panic!("the accent reached a {other:?}"),
                        }
                    }
                }
            }
        }
    }

    /// Judgement 2: a state is not a position.
    ///
    /// A switch that is on stays on inside a section the user has since
    /// collapsed, and unsaved changes stay unsaved while the user is looking at
    /// some other section entirely. Neither is *where you are*, so neither is
    /// drawn in the colour that answers that question — asserted by rendering
    /// the same panel under two different accents and requiring both to come
    /// out unchanged.
    #[test]
    fn a_state_is_not_a_position() {
        for light in [false, true] {
            let a = accented(light);
            let mut b = accented(light);
            b.accent = Color::from_hex(0x00FFFF);
            assert_ne!(
                (a.accent.r, a.accent.g, a.accent.b),
                (b.accent.r, b.accent.g, b.accent.b),
                "the two accents are the same colour, so this proves nothing"
            );
            for (name, role) in b.roles() {
                if name == "accent" {
                    continue;
                }
                assert_ne!(
                    (role.r, role.g, role.b),
                    (b.accent.r, b.accent.g, b.accent.b),
                    "the second accent collides with {name}"
                );
            }

            for accent in [&a, &b] {
                let cmds = panel(2, true, true).render(accent, 0.0, 0.0, PANEL_W);
                assert_eq!(
                    fills_sized(&cmds, 40.0, 20.0),
                    vec![accent.green],
                    "a switch that is on reports a state, not a position"
                );
                assert_eq!(
                    text_color(
                        &cmds,
                        "Unsaved changes — press Apply to save or Revert to discard"
                    ),
                    accent.yellow,
                    "unsaved changes are a fact about the whole panel"
                );
            }

            // And the roles they use are not the accent by coincidence.
            assert_ne!(
                (a.green.r, a.green.g, a.green.b),
                (a.accent.r, a.accent.g, a.accent.b)
            );
            assert_ne!(
                (a.yellow.r, a.yellow.g, a.yellow.b),
                (a.accent.r, a.accent.g, a.accent.b)
            );
        }
    }

    /// Retheming the accent moves the two things in force, and nothing else.
    ///
    /// The other colour tests each look at one site. This one looks at the
    /// whole command list twice and asks which entries differ — the question a
    /// user asks by changing their accent and glancing at the panel. Anything
    /// that moves and is not the open heading, a track fill or a thumb is a
    /// site that has been quietly wired to "where you are" when it reports
    /// something else.
    #[test]
    fn only_what_is_in_force_moves_when_the_accent_moves() {
        for light in [false, true] {
            let a = accented(light);
            let mut b = accented(light);
            b.accent = Color::from_hex(0x00FFFF);

            for section in 0..5 {
                let ca = panel(section, true, true).render(&a, 0.0, 0.0, PANEL_W);
                let cb = panel(section, true, true).render(&b, 0.0, 0.0, PANEL_W);
                assert_eq!(
                    ca.len(),
                    cb.len(),
                    "the accent changed how many commands section {section} draws"
                );
                let mut moved = 0_usize;
                for (i, (x, y)) in ca.iter().zip(cb.iter()).enumerate() {
                    if color_of(x) == color_of(y) {
                        continue;
                    }
                    moved += 1;
                    match x {
                        RenderCommand::Text { text, .. } => assert!(
                            text.starts_with('▼'),
                            "command {i}, {text:?}, follows the accent but is \
                             not the section that is open"
                        ),
                        RenderCommand::FillRect { width, height, .. } => assert!(
                            *height == 6.0 || (*width == 14.0 && *height == 14.0),
                            "command {i} follows the accent but is neither a \
                             slider fill nor its handle"
                        ),
                        other => panic!("command {i}, a {other:?}, follows the accent"),
                    }
                }
                assert!(
                    moved >= 1,
                    "section {section} does not change at all when the accent \
                     does, so nothing on it says which section is open"
                );
            }
        }
    }

    /// Judgement 3: the thumb is derived from the fill, never named beside it.
    ///
    /// It was `LAVENDER` sitting next to a `BLUE` fill — two constants that
    /// looked deliberate only because Catppuccin lists them near each other,
    /// and free to drift apart the moment either was touched. So the claim is
    /// not "the thumb is lavender-ish" but the two halves that make drift
    /// impossible: it *is* [`emphasized`] of the fill, and it is no role at
    /// all, so no future edit can quietly pin it to one.
    #[test]
    fn the_thumb_is_derived_from_the_fill_it_ends() {
        for light in [false, true] {
            for hex in [0x00FF_00FF_u32, 0x0000_FFFF, 0x0000_2200] {
                let mut p = Palette::for_mode(light);
                p.accent = Color::from_hex(hex);
                let cmds = panel(0, false, false).render(&p, 0.0, 0.0, PANEL_W);
                let thumbs = fills_sized(&cmds, 14.0, 14.0);
                assert_eq!(thumbs.len(), 1, "one slider draws one handle");
                assert_eq!(
                    thumbs[0],
                    emphasized(p.accent),
                    "the handle is not derived from the track it ends"
                );
                assert_ne!(
                    (thumbs[0].r, thumbs[0].g, thumbs[0].b),
                    (p.accent.r, p.accent.g, p.accent.b),
                    "the handle is the same colour as the fill, so there is \
                     nothing to grab"
                );
                for (name, role) in p.roles() {
                    assert_ne!(
                        (role.r, role.g, role.b),
                        (thumbs[0].r, thumbs[0].g, thumbs[0].b),
                        "the handle came out equal to {name}, so it is a named \
                         colour after all and free to drift from the fill"
                    );
                }
            }
        }
    }

    /// Judgement 4: exactly one section reads as open, and it is the right one.
    ///
    /// Two of the module's three state-dependent sites are here, and both are
    /// checked in *both* branches at every position — because a header that
    /// picked its colours the other way round, or off by one section, would
    /// still draw one `surface0`, four `mantle`, one accent and four inks. Only
    /// asking *which* section got which can see that.
    #[test]
    fn exactly_one_section_reads_as_open() {
        for light in [false, true] {
            let p = accented(light);
            for open in 0..5 {
                let cmds = panel(open, false, false).render(&p, 0.0, 0.0, PANEL_W);
                let headers = fills_sized(&cmds, INNER_W, 36.0);
                assert_eq!(headers.len(), 5, "one header per section");
                for (i, c) in headers.iter().enumerate() {
                    assert_eq!(
                        *c,
                        if i == open { p.surface0 } else { p.mantle },
                        "the header of section {i} while {open} is open"
                    );
                }
                for (i, name) in InputSettingsUI::SECTIONS.iter().enumerate() {
                    let marker = if i == open { "▼" } else { "▶" };
                    assert_eq!(
                        text_color(&cmds, &format!("{marker} {name}")),
                        if i == open { p.accent } else { p.text },
                        "the heading of section {i} while {open} is open"
                    );
                }
            }
        }
    }

    /// The knob is the same ink on both pills.
    ///
    /// It marks the switch's position, and a position does not change meaning
    /// when the state does — so the one thing the knob must not do is follow
    /// the pill it slides on. That it is *legible* on both is a separate
    /// question and a real defect: `text` on `green` is two light values. See
    /// known-issues.md `TD-C-SWITCH-KNOBS-ARE-LOW-CONTRAST-ON-THE-ON-PILL`,
    /// which is being fixed across the shell in one pass rather than module by
    /// module, so that every switch reaches the same answer.
    #[test]
    fn the_knob_is_the_same_ink_on_both_pills() {
        for light in [false, true] {
            let p = accented(light);
            let off = fills_sized(
                &panel(2, false, false).render(&p, 0.0, 0.0, PANEL_W),
                16.0,
                16.0,
            );
            let on = fills_sized(
                &panel(2, true, false).render(&p, 0.0, 0.0, PANEL_W),
                16.0,
                16.0,
            );
            assert_eq!(off, vec![p.text], "the knob of a switch that is off");
            assert_eq!(on, off, "the knob changes colour with the switch");
        }
    }
}
