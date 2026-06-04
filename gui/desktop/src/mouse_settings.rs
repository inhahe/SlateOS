//! Mouse and keyboard settings panel.
//!
//! Provides a settings UI for configuring mouse pointer speed, acceleration
//! profiles, scroll behaviour, button mapping, double-click speed, and
//! keyboard repeat rate / delay. Integrates with the desktop's Settings app
//! as a sub-page.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Mouse acceleration profile
// ============================================================================

/// Pointer acceleration profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccelProfile {
    /// No acceleration — raw pointer movement.
    Flat,
    /// Adaptive acceleration (faster movement = more acceleration).
    Adaptive,
    /// Custom curve defined by a gain/threshold pair.
    Custom,
}

impl AccelProfile {
    pub fn label(self) -> &'static str {
        match self {
            Self::Flat => "Flat (no acceleration)",
            Self::Adaptive => "Adaptive",
            Self::Custom => "Custom curve",
        }
    }

    pub const ALL: [Self; 3] = [Self::Flat, Self::Adaptive, Self::Custom];
}

// ============================================================================
// Scroll mode
// ============================================================================

/// How scroll events are interpreted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollMode {
    /// Scroll by a fixed number of lines per notch.
    Lines,
    /// Scroll by pages.
    Pages,
    /// Smooth pixel-level scrolling.
    Smooth,
}

impl ScrollMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lines => "Lines",
            Self::Pages => "Pages",
            Self::Smooth => "Smooth",
        }
    }

    pub const ALL: [Self; 3] = [Self::Lines, Self::Pages, Self::Smooth];
}

// ============================================================================
// Button mapping
// ============================================================================

/// Logical mouse button assignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonMapping {
    /// Standard right-handed layout: left=primary, right=secondary.
    RightHanded,
    /// Left-handed: swaps primary and secondary.
    LeftHanded,
}

impl ButtonMapping {
    pub fn label(self) -> &'static str {
        match self {
            Self::RightHanded => "Right-handed",
            Self::LeftHanded => "Left-handed",
        }
    }
}

// ============================================================================
// Keyboard repeat settings
// ============================================================================

/// Keyboard repeat-rate configuration.
#[derive(Clone, Debug)]
pub struct KeyboardRepeatConfig {
    /// Delay before repeat starts, in milliseconds (150–2000).
    pub repeat_delay_ms: u32,
    /// Interval between repeated keystrokes, in milliseconds (10–500).
    pub repeat_interval_ms: u32,
    /// Whether key repeat is enabled at all.
    pub enabled: bool,
}

impl Default for KeyboardRepeatConfig {
    fn default() -> Self {
        Self {
            repeat_delay_ms: 500,
            repeat_interval_ms: 30,
            enabled: true,
        }
    }
}

impl KeyboardRepeatConfig {
    pub fn set_delay(&mut self, ms: u32) {
        self.repeat_delay_ms = ms.clamp(150, 2000);
    }

    pub fn set_interval(&mut self, ms: u32) {
        self.repeat_interval_ms = ms.clamp(10, 500);
    }
}

// ============================================================================
// Mouse settings
// ============================================================================

/// Full mouse configuration.
#[derive(Clone, Debug)]
pub struct MouseConfig {
    /// Pointer speed factor. Range: -10 (slowest) to +10 (fastest). 0 = OS default.
    pub speed: i32,
    /// Acceleration profile.
    pub accel_profile: AccelProfile,
    /// Custom acceleration gain (only used when `accel_profile == Custom`). 0.1–10.0.
    pub accel_gain: f32,
    /// Custom acceleration threshold (only used when `accel_profile == Custom`). 0–50.
    pub accel_threshold: u32,
    /// Button mapping (left- or right-handed).
    pub button_mapping: ButtonMapping,
    /// Double-click speed in milliseconds (100–2000).
    pub double_click_ms: u32,
    /// Scroll mode.
    pub scroll_mode: ScrollMode,
    /// Lines per scroll notch when `scroll_mode == Lines`. 1–20.
    pub scroll_lines: u32,
    /// Scroll speed multiplier for smooth scrolling. 0.1–5.0.
    pub scroll_speed: f32,
    /// Whether to reverse (natural) scrolling direction.
    pub natural_scroll: bool,
    /// Cursor size in pixels (16–128).
    pub cursor_size: u32,
    /// Show a locate animation when Ctrl is pressed.
    pub locate_on_ctrl: bool,
    /// Hide the cursor while typing.
    pub hide_while_typing: bool,
    /// Show a cursor trail.
    pub show_trail: bool,
    /// Trail length (1–10).
    pub trail_length: u32,
}

impl Default for MouseConfig {
    fn default() -> Self {
        Self {
            speed: 0,
            accel_profile: AccelProfile::Adaptive,
            accel_gain: 1.0,
            accel_threshold: 4,
            button_mapping: ButtonMapping::RightHanded,
            double_click_ms: 400,
            scroll_mode: ScrollMode::Lines,
            scroll_lines: 3,
            scroll_speed: 1.0,
            natural_scroll: false,
            cursor_size: 24,
            locate_on_ctrl: false,
            hide_while_typing: false,
            show_trail: false,
            trail_length: 3,
        }
    }
}

impl MouseConfig {
    pub fn set_speed(&mut self, speed: i32) {
        self.speed = speed.clamp(-10, 10);
    }

    pub fn set_double_click_ms(&mut self, ms: u32) {
        self.double_click_ms = ms.clamp(100, 2000);
    }

    pub fn set_scroll_lines(&mut self, lines: u32) {
        self.scroll_lines = lines.clamp(1, 20);
    }

    pub fn set_scroll_speed(&mut self, speed: f32) {
        self.scroll_speed = speed.clamp(0.1, 5.0);
    }

    pub fn set_cursor_size(&mut self, size: u32) {
        self.cursor_size = size.clamp(16, 128);
    }

    pub fn set_trail_length(&mut self, len: u32) {
        self.trail_length = len.clamp(1, 10);
    }

    pub fn set_accel_gain(&mut self, gain: f32) {
        self.accel_gain = gain.clamp(0.1, 10.0);
    }

    pub fn set_accel_threshold(&mut self, thr: u32) {
        self.accel_threshold = thr.min(50);
    }
}

// ============================================================================
// Combined input settings
// ============================================================================

/// Combined mouse + keyboard input settings.
#[derive(Clone, Debug)]
#[derive(Default)]
pub struct InputSettings {
    pub mouse: MouseConfig,
    pub keyboard: KeyboardRepeatConfig,
}


impl InputSettings {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset_mouse(&mut self) {
        self.mouse = MouseConfig::default();
    }

    pub fn reset_keyboard(&mut self) {
        self.keyboard = KeyboardRepeatConfig::default();
    }

    pub fn reset_all(&mut self) {
        *self = Self::default();
    }
}

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
    pub fn render(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
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
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cy += pad;
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: cy,
            text: "Mouse & Keyboard Settings".into(),
            font_size: 20.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(inner),
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
                color: if expanded { SURFACE0 } else { MANTLE },
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + pad + 12.0,
                y: cy + 10.0,
                text: format!("{indicator} {section_name}"),
                font_size: 14.0,
                color: if expanded { BLUE } else { TEXT },
                font_weight: FontWeightHint::Bold,
                max_width: Some(inner - 24.0),
            });
            cy += 40.0;

            if expanded {
                match i {
                    0 => cy = self.render_pointer_section(&mut cmds, x + pad, cy, inner),
                    1 => cy = self.render_buttons_section(&mut cmds, x + pad, cy, inner),
                    2 => cy = self.render_scroll_section(&mut cmds, x + pad, cy, inner),
                    3 => cy = self.render_cursor_section(&mut cmds, x + pad, cy, inner),
                    4 => cy = self.render_keyboard_section(&mut cmds, x + pad, cy, inner),
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
                color: SURFACE0,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + pad + 12.0,
                y: cy + 10.0,
                text: "Unsaved changes — press Apply to save or Revert to discard".into(),
                font_size: 13.0,
                color: YELLOW,
                font_weight: FontWeightHint::Regular,
                max_width: Some(inner - 24.0),
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
        x: f32,
        mut y: f32,
        width: f32,
    ) -> f32 {
        let m = &self.settings.mouse;

        y = self.render_label_value(cmds, x, y, width, "Speed", &format!("{}", m.speed));
        y = self.render_slider_hint(cmds, x, y, width, -10, 10, m.speed);

        y = self.render_label_value(
            cmds,
            x,
            y,
            width,
            "Acceleration profile",
            m.accel_profile.label(),
        );

        if m.accel_profile == AccelProfile::Custom {
            y = self.render_label_value(
                cmds,
                x,
                y,
                width,
                "Accel gain",
                &format!("{:.1}", m.accel_gain),
            );
            y = self.render_label_value(
                cmds,
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
        x: f32,
        mut y: f32,
        width: f32,
    ) -> f32 {
        let m = &self.settings.mouse;

        y = self.render_label_value(cmds, x, y, width, "Button layout", m.button_mapping.label());
        y = self.render_label_value(
            cmds,
            x,
            y,
            width,
            "Double-click speed",
            &format!("{} ms", m.double_click_ms),
        );
        y = self.render_slider_hint(cmds, x, y, width, 100, 2000, m.double_click_ms as i32);

        y
    }

    fn render_scroll_section(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) -> f32 {
        let m = &self.settings.mouse;

        y = self.render_label_value(cmds, x, y, width, "Scroll mode", m.scroll_mode.label());

        if m.scroll_mode == ScrollMode::Lines {
            y = self.render_label_value(
                cmds,
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
                x,
                y,
                width,
                "Scroll speed",
                &format!("{:.1}×", m.scroll_speed),
            );
        }

        y = self.render_toggle(cmds, x, y, width, "Natural scroll", m.natural_scroll);

        y
    }

    fn render_cursor_section(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) -> f32 {
        let m = &self.settings.mouse;

        y = self.render_label_value(
            cmds,
            x,
            y,
            width,
            "Cursor size",
            &format!("{} px", m.cursor_size),
        );
        y = self.render_toggle(cmds, x, y, width, "Locate on Ctrl press", m.locate_on_ctrl);
        y = self.render_toggle(cmds, x, y, width, "Hide while typing", m.hide_while_typing);
        y = self.render_toggle(cmds, x, y, width, "Show cursor trail", m.show_trail);

        if m.show_trail {
            y = self.render_label_value(
                cmds,
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
        x: f32,
        mut y: f32,
        width: f32,
    ) -> f32 {
        let k = &self.settings.keyboard;

        y = self.render_toggle(cmds, x, y, width, "Key repeat enabled", k.enabled);

        if k.enabled {
            y = self.render_label_value(
                cmds,
                x,
                y,
                width,
                "Repeat delay",
                &format!("{} ms", k.repeat_delay_ms),
            );
            y = self.render_slider_hint(cmds, x, y, width, 150, 2000, k.repeat_delay_ms as i32);

            y = self.render_label_value(
                cmds,
                x,
                y,
                width,
                "Repeat interval",
                &format!("{} ms", k.repeat_interval_ms),
            );
            y = self.render_slider_hint(cmds, x, y, width, 10, 500, k.repeat_interval_ms as i32);
        }

        y
    }

    // ------------------------------------------------------------------
    // Shared rendering helpers
    // ------------------------------------------------------------------

    fn render_label_value(
        &self,
        cmds: &mut Vec<RenderCommand>,
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
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.55,
            y,
            text: value.into(),
            font_size: 13.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.4),
        });
        y + 22.0
    }

    fn render_toggle(
        &self,
        cmds: &mut Vec<RenderCommand>,
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
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.6),
        });

        // Toggle pill
        let tx = x + width - 48.0;
        let bg = if on { GREEN } else { SURFACE1 };
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
            color: TEXT,
            corner_radii: CornerRadii::all(8.0),
        });

        y + 26.0
    }

    fn render_slider_hint(
        &self,
        cmds: &mut Vec<RenderCommand>,
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
            color: SURFACE1,
            corner_radii: CornerRadii::all(3.0),
        });

        // Fill
        let range = (max - min).max(1) as f32;
        let frac = ((value - min) as f32 / range).clamp(0.0, 1.0);
        cmds.push(RenderCommand::FillRect {
            x: track_x,
            y: y + 4.0,
            width: track_w * frac,
            height: track_h,
            color: BLUE,
            corner_radii: CornerRadii::all(3.0),
        });

        // Thumb
        let thumb_x = track_x + track_w * frac - 7.0;
        cmds.push(RenderCommand::FillRect {
            x: thumb_x,
            y,
            width: 14.0,
            height: 14.0,
            color: LAVENDER,
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mouse_config() {
        let c = MouseConfig::default();
        assert_eq!(c.speed, 0);
        assert_eq!(c.accel_profile, AccelProfile::Adaptive);
        assert_eq!(c.button_mapping, ButtonMapping::RightHanded);
        assert_eq!(c.double_click_ms, 400);
        assert_eq!(c.scroll_mode, ScrollMode::Lines);
        assert_eq!(c.scroll_lines, 3);
        assert!(!c.natural_scroll);
        assert_eq!(c.cursor_size, 24);
    }

    #[test]
    fn speed_clamped() {
        let mut c = MouseConfig::default();
        c.set_speed(-20);
        assert_eq!(c.speed, -10);
        c.set_speed(100);
        assert_eq!(c.speed, 10);
    }

    #[test]
    fn double_click_clamped() {
        let mut c = MouseConfig::default();
        c.set_double_click_ms(5);
        assert_eq!(c.double_click_ms, 100);
        c.set_double_click_ms(9999);
        assert_eq!(c.double_click_ms, 2000);
    }

    #[test]
    fn scroll_lines_clamped() {
        let mut c = MouseConfig::default();
        c.set_scroll_lines(0);
        assert_eq!(c.scroll_lines, 1);
        c.set_scroll_lines(100);
        assert_eq!(c.scroll_lines, 20);
    }

    #[test]
    fn scroll_speed_clamped() {
        let mut c = MouseConfig::default();
        c.set_scroll_speed(0.0);
        assert!((c.scroll_speed - 0.1).abs() < 0.01);
        c.set_scroll_speed(100.0);
        assert!((c.scroll_speed - 5.0).abs() < 0.01);
    }

    #[test]
    fn cursor_size_clamped() {
        let mut c = MouseConfig::default();
        c.set_cursor_size(5);
        assert_eq!(c.cursor_size, 16);
        c.set_cursor_size(500);
        assert_eq!(c.cursor_size, 128);
    }

    #[test]
    fn trail_length_clamped() {
        let mut c = MouseConfig::default();
        c.set_trail_length(0);
        assert_eq!(c.trail_length, 1);
        c.set_trail_length(99);
        assert_eq!(c.trail_length, 10);
    }

    #[test]
    fn accel_gain_clamped() {
        let mut c = MouseConfig::default();
        c.set_accel_gain(0.0);
        assert!((c.accel_gain - 0.1).abs() < 0.01);
        c.set_accel_gain(50.0);
        assert!((c.accel_gain - 10.0).abs() < 0.01);
    }

    #[test]
    fn accel_threshold_clamped() {
        let mut c = MouseConfig::default();
        c.set_accel_threshold(100);
        assert_eq!(c.accel_threshold, 50);
    }

    #[test]
    fn default_keyboard_config() {
        let k = KeyboardRepeatConfig::default();
        assert_eq!(k.repeat_delay_ms, 500);
        assert_eq!(k.repeat_interval_ms, 30);
        assert!(k.enabled);
    }

    #[test]
    fn keyboard_delay_clamped() {
        let mut k = KeyboardRepeatConfig::default();
        k.set_delay(10);
        assert_eq!(k.repeat_delay_ms, 150);
        k.set_delay(9999);
        assert_eq!(k.repeat_delay_ms, 2000);
    }

    #[test]
    fn keyboard_interval_clamped() {
        let mut k = KeyboardRepeatConfig::default();
        k.set_interval(1);
        assert_eq!(k.repeat_interval_ms, 10);
        k.set_interval(9999);
        assert_eq!(k.repeat_interval_ms, 500);
    }

    #[test]
    fn input_settings_reset() {
        let mut s = InputSettings::new();
        s.mouse.set_speed(5);
        s.keyboard.set_delay(1000);
        s.reset_mouse();
        assert_eq!(s.mouse.speed, 0);
        assert_eq!(s.keyboard.repeat_delay_ms, 1000);
        s.reset_all();
        assert_eq!(s.keyboard.repeat_delay_ms, 500);
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
        let cmds = ui.render(0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn ui_render_with_dirty() {
        let mut ui = InputSettingsUI::new();
        ui.settings_mut().mouse.set_speed(3);
        let cmds = ui.render(0.0, 0.0, 400.0);
        // Should contain a yellow "unsaved changes" text.
        let has_yellow = cmds.iter().any(|c| matches!(c, RenderCommand::Text { color, .. } if *color == YELLOW));
        assert!(has_yellow);
    }

    #[test]
    fn ui_render_each_section() {
        let mut ui = InputSettingsUI::new();
        for i in 0..5 {
            ui.expand_section(i);
            let cmds = ui.render(0.0, 0.0, 400.0);
            assert!(!cmds.is_empty());
        }
    }

    #[test]
    fn ui_custom_accel_shows_extra_fields() {
        let mut ui = InputSettingsUI::new();
        ui.settings_mut().mouse.accel_profile = AccelProfile::Custom;
        ui.expand_section(0);
        let cmds = ui.render(0.0, 0.0, 400.0);
        let has_gain = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Accel gain")));
        assert!(has_gain);
    }

    #[test]
    fn accel_profile_labels() {
        for p in AccelProfile::ALL {
            assert!(!p.label().is_empty());
        }
    }

    #[test]
    fn scroll_mode_labels() {
        for m in ScrollMode::ALL {
            assert!(!m.label().is_empty());
        }
    }

    #[test]
    fn button_mapping_labels() {
        assert!(!ButtonMapping::RightHanded.label().is_empty());
        assert!(!ButtonMapping::LeftHanded.label().is_empty());
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
    fn natural_scroll_toggle() {
        let mut c = MouseConfig::default();
        assert!(!c.natural_scroll);
        c.natural_scroll = true;
        assert!(c.natural_scroll);
    }

    #[test]
    fn show_trail_toggle_renders_trail_length() {
        let mut ui = InputSettingsUI::new();
        ui.settings_mut().mouse.show_trail = true;
        ui.expand_section(3);
        let cmds = ui.render(0.0, 0.0, 400.0);
        let has_trail = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Trail length")));
        assert!(has_trail);
    }

    #[test]
    fn keyboard_disabled_hides_sliders() {
        let mut ui = InputSettingsUI::new();
        ui.settings_mut().keyboard.enabled = false;
        ui.expand_section(4);
        let cmds = ui.render(0.0, 0.0, 400.0);
        let has_delay = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Repeat delay")));
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
        let cmds = ui.render(0.0, 0.0, 400.0);
        let has_speed = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Scroll speed")));
        assert!(has_speed);
    }

    #[test]
    fn lines_scroll_shows_lines_per_notch() {
        let mut ui = InputSettingsUI::new();
        ui.settings_mut().mouse.scroll_mode = ScrollMode::Lines;
        ui.expand_section(2);
        let cmds = ui.render(0.0, 0.0, 400.0);
        let has_lines = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Lines per notch")));
        assert!(has_lines);
    }
}
