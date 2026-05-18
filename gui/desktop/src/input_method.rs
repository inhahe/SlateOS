//! Input method and keyboard layout management module.
//!
//! Provides:
//! - Keyboard layout switching (e.g., US QWERTY, UK, Dvorak, Colemak, etc.)
//! - Input method indicator in the system tray
//! - Layout preview (visual keyboard showing the current layout)
//! - Keyboard shortcut for switching layouts (Alt+Shift or Super+Space)
//! - Per-application layout memory
//! - Dead key / compose key support tracking
//! - Custom layout support

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Theme
// ============================================================================

const MOCHA_BASE: Color = Color::from_hex(0x1E1E2E);
const MOCHA_SURFACE0: Color = Color::from_hex(0x313244);
const MOCHA_SURFACE1: Color = Color::from_hex(0x45475A);
const MOCHA_TEXT: Color = Color::from_hex(0xCDD6F4);
const MOCHA_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const MOCHA_BLUE: Color = Color::from_hex(0x89B4FA);
const MOCHA_OVERLAY0: Color = Color::from_hex(0x6C7086);
const MOCHA_MANTLE: Color = Color::from_hex(0x181825);

// ============================================================================
// Keyboard layouts
// ============================================================================

/// Identifier for a keyboard layout.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LayoutId(pub String);

impl LayoutId {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }
}

/// A keyboard layout definition.
#[derive(Clone, Debug)]
pub struct KeyboardLayout {
    /// Unique identifier (e.g., "us-qwerty", "uk", "dvorak").
    pub id: LayoutId,
    /// Display name (e.g., "US English (QWERTY)").
    pub display_name: String,
    /// Short label for the tray indicator (e.g., "EN", "UK", "DV").
    pub short_label: String,
    /// Language code (e.g., "en", "de", "fr").
    pub language: String,
    /// Whether this layout uses dead keys (accents).
    pub has_dead_keys: bool,
    /// Whether this is a right-to-left layout.
    pub is_rtl: bool,
    /// Key mapping for the main 4 rows (unshifted). Each row is a string
    /// where each char is the key at that position.
    pub rows_unshifted: [String; 4],
    /// Key mapping for shifted state.
    pub rows_shifted: [String; 4],
}

impl KeyboardLayout {
    /// Create the US QWERTY layout.
    pub fn us_qwerty() -> Self {
        Self {
            id: LayoutId::new("us-qwerty"),
            display_name: "US English (QWERTY)".to_string(),
            short_label: "EN".to_string(),
            language: "en".to_string(),
            has_dead_keys: false,
            is_rtl: false,
            rows_unshifted: [
                "`1234567890-=".to_string(),
                "qwertyuiop[]\\".to_string(),
                "asdfghjkl;'".to_string(),
                "zxcvbnm,./".to_string(),
            ],
            rows_shifted: [
                "~!@#$%^&*()_+".to_string(),
                "QWERTYUIOP{}|".to_string(),
                "ASDFGHJKL:\"".to_string(),
                "ZXCVBNM<>?".to_string(),
            ],
        }
    }

    /// Create the Dvorak layout.
    pub fn dvorak() -> Self {
        Self {
            id: LayoutId::new("dvorak"),
            display_name: "Dvorak".to_string(),
            short_label: "DV".to_string(),
            language: "en".to_string(),
            has_dead_keys: false,
            is_rtl: false,
            rows_unshifted: [
                "`1234567890[]".to_string(),
                "',.pyfgcrl/=\\".to_string(),
                "aoeuidhtns-".to_string(),
                ";qjkxbmwvz".to_string(),
            ],
            rows_shifted: [
                "~!@#$%^&*(){}".to_string(),
                "\"<>PYFGCRL?+|".to_string(),
                "AOEUIDHTNS_".to_string(),
                ":QJKXBMWVZ".to_string(),
            ],
        }
    }

    /// Create the Colemak layout.
    pub fn colemak() -> Self {
        Self {
            id: LayoutId::new("colemak"),
            display_name: "Colemak".to_string(),
            short_label: "CO".to_string(),
            language: "en".to_string(),
            has_dead_keys: false,
            is_rtl: false,
            rows_unshifted: [
                "`1234567890-=".to_string(),
                "qwfpgjluy;[]\\".to_string(),
                "arstdhneio'".to_string(),
                "zxcvbkm,./".to_string(),
            ],
            rows_shifted: [
                "~!@#$%^&*()_+".to_string(),
                "QWFPGJLUY:{}|".to_string(),
                "ARSTDHNEIO\"".to_string(),
                "ZXCVBKM<>?".to_string(),
            ],
        }
    }

    /// Create German QWERTZ layout.
    pub fn german_qwertz() -> Self {
        Self {
            id: LayoutId::new("de-qwertz"),
            display_name: "German (QWERTZ)".to_string(),
            short_label: "DE".to_string(),
            language: "de".to_string(),
            has_dead_keys: true,
            is_rtl: false,
            rows_unshifted: [
                "^1234567890ß´".to_string(),
                "qwertzuiopü+".to_string(),
                "asdfghjklöä#".to_string(),
                "<yxcvbnm,.-".to_string(),
            ],
            rows_shifted: [
                "°!\"§$%&/()=?`".to_string(),
                "QWERTZUIOPÜ*".to_string(),
                "ASDFGHJKLÖÄ'".to_string(),
                ">YXCVBNM;:_".to_string(),
            ],
        }
    }

    /// Create French AZERTY layout.
    pub fn french_azerty() -> Self {
        Self {
            id: LayoutId::new("fr-azerty"),
            display_name: "French (AZERTY)".to_string(),
            short_label: "FR".to_string(),
            language: "fr".to_string(),
            has_dead_keys: true,
            is_rtl: false,
            rows_unshifted: [
                "²&é\"'(-è_çà)=".to_string(),
                "azertyuiop^$".to_string(),
                "qsdfghjklmù*".to_string(),
                "<wxcvbn,;:!".to_string(),
            ],
            rows_shifted: [
                " 1234567890°+".to_string(),
                "AZERTYUIOP¨£".to_string(),
                "QSDFGHJKLM%µ".to_string(),
                ">WXCVBN?./§".to_string(),
            ],
        }
    }

    /// All built-in layouts.
    pub fn all_builtins() -> Vec<Self> {
        vec![
            Self::us_qwerty(),
            Self::dvorak(),
            Self::colemak(),
            Self::german_qwertz(),
            Self::french_azerty(),
        ]
    }
}

// ============================================================================
// Input method manager
// ============================================================================

/// Shortcut for switching keyboard layouts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchShortcut {
    AltShift,
    CtrlShift,
    SuperSpace,
}

impl SwitchShortcut {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::AltShift => "Alt + Shift",
            Self::CtrlShift => "Ctrl + Shift",
            Self::SuperSpace => "Super + Space",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::AltShift => "alt-shift",
            Self::CtrlShift => "ctrl-shift",
            Self::SuperSpace => "super-space",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s {
            "alt-shift" => Self::AltShift,
            "ctrl-shift" => Self::CtrlShift,
            "super-space" => Self::SuperSpace,
            _ => Self::AltShift,
        }
    }
}

/// Manages installed layouts and the active layout.
#[derive(Clone, Debug)]
pub struct InputMethodManager {
    /// Installed layouts (in switching order).
    pub layouts: Vec<KeyboardLayout>,
    /// Index of the currently active layout.
    pub active_index: usize,
    /// Shortcut for cycling layouts.
    pub switch_shortcut: SwitchShortcut,
    /// Whether to remember layout per application.
    pub per_app_layout: bool,
    /// Per-app layout memory: app_id → layout index.
    pub app_layouts: Vec<(String, usize)>,
    /// Whether the layout preview popup is visible.
    pub preview_visible: bool,
}

impl Default for InputMethodManager {
    fn default() -> Self {
        Self {
            layouts: vec![KeyboardLayout::us_qwerty()],
            active_index: 0,
            switch_shortcut: SwitchShortcut::AltShift,
            per_app_layout: false,
            app_layouts: Vec::new(),
            preview_visible: false,
        }
    }
}

impl InputMethodManager {
    /// Create with specific layouts.
    pub fn new(layouts: Vec<KeyboardLayout>) -> Self {
        Self {
            layouts,
            ..Self::default()
        }
    }

    /// Get the active layout.
    pub fn active_layout(&self) -> Option<&KeyboardLayout> {
        self.layouts.get(self.active_index)
    }

    /// Get the short label for the tray indicator.
    pub fn tray_label(&self) -> &str {
        self.active_layout()
            .map(|l| l.short_label.as_str())
            .unwrap_or("??")
    }

    /// Cycle to the next layout.
    pub fn next_layout(&mut self) {
        if !self.layouts.is_empty() {
            self.active_index = (self.active_index + 1) % self.layouts.len();
        }
    }

    /// Cycle to the previous layout.
    pub fn prev_layout(&mut self) {
        if !self.layouts.is_empty() {
            self.active_index = if self.active_index == 0 {
                self.layouts.len() - 1
            } else {
                self.active_index - 1
            };
        }
    }

    /// Switch to a specific layout by id.
    pub fn switch_to(&mut self, id: &LayoutId) -> bool {
        if let Some(idx) = self.layouts.iter().position(|l| l.id == *id) {
            self.active_index = idx;
            true
        } else {
            false
        }
    }

    /// Add a layout (if not already installed).
    pub fn add_layout(&mut self, layout: KeyboardLayout) -> bool {
        if self.layouts.iter().any(|l| l.id == layout.id) {
            return false;
        }
        self.layouts.push(layout);
        true
    }

    /// Remove a layout by id. Cannot remove the last layout.
    pub fn remove_layout(&mut self, id: &LayoutId) -> bool {
        if self.layouts.len() <= 1 {
            return false;
        }
        if let Some(idx) = self.layouts.iter().position(|l| l.id == *id) {
            self.layouts.remove(idx);
            if self.active_index >= self.layouts.len() {
                self.active_index = self.layouts.len() - 1;
            }
            true
        } else {
            false
        }
    }

    /// Notify that the active application changed. If per-app layout is
    /// enabled, switch to the remembered layout for this app.
    pub fn on_app_focus(&mut self, app_id: &str) {
        if !self.per_app_layout {
            return;
        }
        if let Some(idx) = self
            .app_layouts
            .iter()
            .find(|(id, _)| id == app_id)
            .map(|(_, idx)| *idx)
        {
            if idx < self.layouts.len() {
                self.active_index = idx;
            }
        }
    }

    /// Remember the current layout for an application.
    pub fn remember_for_app(&mut self, app_id: &str) {
        if let Some(entry) = self.app_layouts.iter_mut().find(|(id, _)| id == app_id) {
            entry.1 = self.active_index;
        } else {
            self.app_layouts
                .push((app_id.to_string(), self.active_index));
        }
    }

    /// Toggle the layout preview popup.
    pub fn toggle_preview(&mut self) {
        self.preview_visible = !self.preview_visible;
    }

    /// Render the tray indicator (small label showing current layout).
    pub fn render_tray_indicator(&self, x: f32, y: f32) -> Vec<RenderCommand> {
        let label = self.tray_label();

        vec![
            RenderCommand::FillRect {
                x,
                y,
                width: 28.0,
                height: 20.0,
                color: MOCHA_SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            },
            RenderCommand::Text {
                x: x + 4.0,
                y: y + 3.0,
                text: label.to_string(),
                font_size: 11.0,
                color: MOCHA_TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(24.0),
            },
        ]
    }

    /// Render the keyboard layout preview popup.
    pub fn render_preview(
        &self,
        popup_x: f32,
        popup_y: f32,
        width: f32,
    ) -> Vec<RenderCommand> {
        if !self.preview_visible {
            return Vec::new();
        }

        let layout = match self.active_layout() {
            Some(l) => l,
            None => return Vec::new(),
        };

        let mut cmds = Vec::with_capacity(80);
        let height = 200.0;

        // Background
        cmds.push(RenderCommand::FillRect {
            x: popup_x,
            y: popup_y,
            width,
            height,
            color: MOCHA_BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Border
        cmds.push(RenderCommand::StrokeRect {
            x: popup_x,
            y: popup_y,
            width,
            height,
            color: MOCHA_SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: popup_x + 12.0,
            y: popup_y + 8.0,
            text: layout.display_name.clone(),
            font_size: 13.0,
            color: MOCHA_BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 24.0),
        });

        // Render keyboard rows
        let key_size = 28.0;
        let key_gap = 3.0;
        let row_offsets = [0.0_f32, 12.0, 20.0, 32.0]; // Stagger offsets
        let start_y = popup_y + 32.0;

        for (row_idx, row_chars) in layout.rows_unshifted.iter().enumerate() {
            let row_y = start_y + row_idx as f32 * (key_size + key_gap);
            let stagger = row_offsets.get(row_idx).copied().unwrap_or(0.0);

            for (col_idx, ch) in row_chars.chars().enumerate() {
                let key_x = popup_x + 8.0 + stagger + col_idx as f32 * (key_size + key_gap);

                // Key background
                cmds.push(RenderCommand::FillRect {
                    x: key_x,
                    y: row_y,
                    width: key_size,
                    height: key_size,
                    color: MOCHA_SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });

                // Key label
                let mut label = String::with_capacity(4);
                label.push(ch);
                cmds.push(RenderCommand::Text {
                    x: key_x + 6.0,
                    y: row_y + 6.0,
                    text: label,
                    font_size: 12.0,
                    color: MOCHA_TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(key_size - 4.0),
                });
            }
        }

        cmds
    }

    /// Serialize config to text.
    pub fn to_config_text(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str("# Input method config\n");
        out.push_str(&format!("switch_shortcut={}\n", self.switch_shortcut.id()));
        out.push_str(&format!("per_app_layout={}\n", self.per_app_layout));

        for (i, layout) in self.layouts.iter().enumerate() {
            out.push_str(&format!("layout_{}={}\n", i, layout.id.0));
        }
        out.push_str(&format!("active={}\n", self.active_index));

        out
    }

    /// Parse config from text (only reads shortcut and active index; layouts
    /// must be resolved separately).
    pub fn apply_config_text(&mut self, text: &str) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                match key.trim() {
                    "switch_shortcut" => {
                        self.switch_shortcut = SwitchShortcut::from_id(val.trim());
                    }
                    "per_app_layout" => {
                        self.per_app_layout = val.trim() == "true";
                    }
                    "active" => {
                        if let Ok(idx) = val.trim().parse::<usize>() {
                            if idx < self.layouts.len() {
                                self.active_index = idx;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- KeyboardLayout tests ----

    #[test]
    fn test_us_qwerty_layout() {
        let l = KeyboardLayout::us_qwerty();
        assert_eq!(l.short_label, "EN");
        assert!(!l.has_dead_keys);
        assert!(!l.is_rtl);
        assert!(l.rows_unshifted[1].contains('q'));
    }

    #[test]
    fn test_dvorak_layout() {
        let l = KeyboardLayout::dvorak();
        assert_eq!(l.short_label, "DV");
        // In Dvorak, top row starts with ',.p
        assert!(l.rows_unshifted[1].starts_with("',.p"));
    }

    #[test]
    fn test_colemak_layout() {
        let l = KeyboardLayout::colemak();
        assert_eq!(l.short_label, "CO");
    }

    #[test]
    fn test_german_layout_has_dead_keys() {
        let l = KeyboardLayout::german_qwertz();
        assert!(l.has_dead_keys);
        assert_eq!(l.language, "de");
    }

    #[test]
    fn test_french_layout() {
        let l = KeyboardLayout::french_azerty();
        assert_eq!(l.short_label, "FR");
        // AZERTY: first row starts with 'a'
        assert!(l.rows_unshifted[1].starts_with('a'));
    }

    #[test]
    fn test_all_builtins_count() {
        let builtins = KeyboardLayout::all_builtins();
        assert_eq!(builtins.len(), 5);
    }

    // ---- SwitchShortcut tests ----

    #[test]
    fn test_shortcut_roundtrip() {
        for s in [
            SwitchShortcut::AltShift,
            SwitchShortcut::CtrlShift,
            SwitchShortcut::SuperSpace,
        ] {
            assert_eq!(SwitchShortcut::from_id(s.id()), s);
        }
    }

    #[test]
    fn test_shortcut_unknown_defaults() {
        assert_eq!(
            SwitchShortcut::from_id("unknown"),
            SwitchShortcut::AltShift
        );
    }

    // ---- InputMethodManager tests ----

    #[test]
    fn test_manager_default() {
        let mgr = InputMethodManager::default();
        assert_eq!(mgr.layouts.len(), 1);
        assert_eq!(mgr.active_index, 0);
        assert_eq!(mgr.tray_label(), "EN");
    }

    #[test]
    fn test_manager_next_layout() {
        let mut mgr = InputMethodManager::new(vec![
            KeyboardLayout::us_qwerty(),
            KeyboardLayout::dvorak(),
        ]);
        assert_eq!(mgr.tray_label(), "EN");
        mgr.next_layout();
        assert_eq!(mgr.tray_label(), "DV");
        mgr.next_layout();
        assert_eq!(mgr.tray_label(), "EN"); // wraps
    }

    #[test]
    fn test_manager_prev_layout() {
        let mut mgr = InputMethodManager::new(vec![
            KeyboardLayout::us_qwerty(),
            KeyboardLayout::dvorak(),
            KeyboardLayout::colemak(),
        ]);
        mgr.prev_layout();
        assert_eq!(mgr.tray_label(), "CO"); // wraps to last
    }

    #[test]
    fn test_manager_switch_to() {
        let mut mgr = InputMethodManager::new(vec![
            KeyboardLayout::us_qwerty(),
            KeyboardLayout::dvorak(),
        ]);
        assert!(mgr.switch_to(&LayoutId::new("dvorak")));
        assert_eq!(mgr.tray_label(), "DV");
    }

    #[test]
    fn test_manager_switch_to_nonexistent() {
        let mut mgr = InputMethodManager::default();
        assert!(!mgr.switch_to(&LayoutId::new("nonexistent")));
    }

    #[test]
    fn test_manager_add_layout() {
        let mut mgr = InputMethodManager::default();
        assert!(mgr.add_layout(KeyboardLayout::dvorak()));
        assert_eq!(mgr.layouts.len(), 2);
    }

    #[test]
    fn test_manager_add_duplicate_fails() {
        let mut mgr = InputMethodManager::default();
        assert!(!mgr.add_layout(KeyboardLayout::us_qwerty()));
    }

    #[test]
    fn test_manager_remove_layout() {
        let mut mgr = InputMethodManager::new(vec![
            KeyboardLayout::us_qwerty(),
            KeyboardLayout::dvorak(),
        ]);
        assert!(mgr.remove_layout(&LayoutId::new("dvorak")));
        assert_eq!(mgr.layouts.len(), 1);
    }

    #[test]
    fn test_manager_remove_last_fails() {
        let mut mgr = InputMethodManager::default();
        assert!(!mgr.remove_layout(&LayoutId::new("us-qwerty")));
    }

    #[test]
    fn test_manager_remove_adjusts_active_index() {
        let mut mgr = InputMethodManager::new(vec![
            KeyboardLayout::us_qwerty(),
            KeyboardLayout::dvorak(),
        ]);
        mgr.active_index = 1;
        mgr.remove_layout(&LayoutId::new("dvorak"));
        assert_eq!(mgr.active_index, 0);
    }

    #[test]
    fn test_manager_per_app_layout() {
        let mut mgr = InputMethodManager::new(vec![
            KeyboardLayout::us_qwerty(),
            KeyboardLayout::dvorak(),
        ]);
        mgr.per_app_layout = true;

        // Set dvorak for terminal
        mgr.active_index = 1;
        mgr.remember_for_app("terminal");

        // Switch to us-qwerty for editor
        mgr.active_index = 0;
        mgr.remember_for_app("editor");

        // Switching apps restores layout
        mgr.on_app_focus("terminal");
        assert_eq!(mgr.active_index, 1);

        mgr.on_app_focus("editor");
        assert_eq!(mgr.active_index, 0);
    }

    #[test]
    fn test_manager_per_app_disabled() {
        let mut mgr = InputMethodManager::new(vec![
            KeyboardLayout::us_qwerty(),
            KeyboardLayout::dvorak(),
        ]);
        mgr.per_app_layout = false;
        mgr.active_index = 0;
        mgr.on_app_focus("anything");
        assert_eq!(mgr.active_index, 0); // No change
    }

    #[test]
    fn test_manager_toggle_preview() {
        let mut mgr = InputMethodManager::default();
        assert!(!mgr.preview_visible);
        mgr.toggle_preview();
        assert!(mgr.preview_visible);
        mgr.toggle_preview();
        assert!(!mgr.preview_visible);
    }

    #[test]
    fn test_manager_render_tray() {
        let mgr = InputMethodManager::default();
        let cmds = mgr.render_tray_indicator(100.0, 50.0);
        assert_eq!(cmds.len(), 2); // bg rect + text
    }

    #[test]
    fn test_manager_render_preview_hidden() {
        let mgr = InputMethodManager::default();
        let cmds = mgr.render_preview(0.0, 0.0, 400.0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_manager_render_preview_visible() {
        let mut mgr = InputMethodManager::default();
        mgr.preview_visible = true;
        let cmds = mgr.render_preview(0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_manager_config_roundtrip() {
        let mut mgr = InputMethodManager::new(vec![
            KeyboardLayout::us_qwerty(),
            KeyboardLayout::dvorak(),
        ]);
        mgr.switch_shortcut = SwitchShortcut::SuperSpace;
        mgr.per_app_layout = true;
        mgr.active_index = 1;

        let text = mgr.to_config_text();
        let mut mgr2 = InputMethodManager::new(vec![
            KeyboardLayout::us_qwerty(),
            KeyboardLayout::dvorak(),
        ]);
        mgr2.apply_config_text(&text);

        assert_eq!(mgr2.switch_shortcut, SwitchShortcut::SuperSpace);
        assert!(mgr2.per_app_layout);
        assert_eq!(mgr2.active_index, 1);
    }

    #[test]
    fn test_empty_manager_tray_label() {
        let mgr = InputMethodManager {
            layouts: Vec::new(),
            active_index: 0,
            ..InputMethodManager::default()
        };
        assert_eq!(mgr.tray_label(), "??");
    }

    #[test]
    fn test_next_layout_empty() {
        let mut mgr = InputMethodManager {
            layouts: Vec::new(),
            active_index: 0,
            ..InputMethodManager::default()
        };
        mgr.next_layout(); // Should not panic
    }
}
