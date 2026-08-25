//! Input method and keyboard layout management module.
//!
//! Provides:
//! - Keyboard layout switching (e.g., US QWERTY, UK, Dvorak, Colemak, etc.)
//! - Input method indicator in the system tray
//! - Layout preview (visual keyboard showing the current layout)
//! - Keyboard shortcut for switching layouts (Alt+Shift or Super+Space)
//! - Per-application layout memory
//! - Custom layout support
//!
//! # What this module is, and what it is not
//!
//! It is a **selector**, not a keymap. Choosing a layout here says which of
//! [`keylayout`]'s tables the user wants; *applying* that choice — turning a
//! scancode into a letter — happens in the compositor, because that is the one
//! process every client's keystrokes pass through (`design-decisions.md`
//! §456). The path from here to there runs through the settings file:
//! `inputsettings`' `keyboard.layout`, which the compositor resolves on
//! reload.
//!
//! This module used to carry its own copies of the layout tables — four
//! unshifted and four shifted row strings per layout, written out by hand
//! beside a second set in the compositor that had no letters in it at all. The
//! two described one keyboard and could not be made to agree: the compositor's
//! was what you typed with, and this one was only ever what you were *shown*,
//! so picking Dvorak in the tray redrew the preview and changed nothing you
//! typed. They are now one table, read from here for the picture and from the
//! compositor for the letters. See `known-issues.md`
//! `TD-ONLY-ONE-KEYBOARD-LAYOUT`.
//!
//! Dead keys and compose sequences are not modelled anywhere yet — the layouts
//! reachable here are the ones that need none. Tracking a `has_dead_keys` flag
//! that no code consulted was worse than not tracking it: it read as support.

use appearance::Palette;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::step;
use guitk::style::CornerRadii;
use keylayout::Layout;

// ============================================================================
// Colour
// ============================================================================
//
// Seven sites, and — unusually for this shell — not one of them is the accent.
// The accent means "you chose this" (see `focus_assist`'s settings panel, which
// spells the rule out), and it earns its keep where a chosen thing sits beside
// unchosen siblings. Nothing here has siblings on screen: the tray shows one
// label, and the preview popup shows one keyboard. A lone accented heading
// would be decoration wearing the vocabulary of choice.
//
// **The preview's title was `MOCHA_BLUE`, and that was the default accent in
// disguise.** Blue is what `Palette::accent` resolves to on a fresh install, so
// the title read as a properly accented heading for as long as nobody visited
// Appearance — and became a stray blue that matched nothing else on screen the
// moment somebody picked Green. This is `backup_settings`'s `InProgress => blue`
// trap arriving from the other direction: there, a semantic hue would have
// collapsed onto the accent; here, an accent-shaped decision was made by
// writing the accent's *current value*. The title is now `p.text` bold, which is
// what every other panel title in the shell is.
//
// The remaining six are neutral by construction and both clear the contrast
// floor in both modes: the tray label and the key caps are `text` on `surface0`
// (8.69 Mocha / 5.17 Latte), and the popup is `base` behind a `surface1` hairline
// with `text` on it (11.34 / 7.06). `surface0` for the key caps is the one place
// a *fill* role is load-bearing rather than decorative — the caps have to read as
// raised against the popup's `base`, and `surface1` would put them at the same
// value as the border that frames them.

/// Side of one key cap in the layout preview. Named so the geometry tests can
/// find a cap by its size rather than by its colour — a layout test that
/// locates by colour silently asserts a role, and stops working the moment the
/// role changes.
const KEY_SIZE: f32 = 28.0;

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
    ///
    /// Owned copies rather than `&'static` references to the built-ins, so
    /// that a layout the user wrote themselves sits in this list on the same
    /// terms as `keylayout`'s own. A `Layout` is forty-odd `Copy` records; the
    /// list is built once when the shell starts.
    pub layouts: Vec<Layout>,
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
            layouts: vec![keylayout::default_layout().clone()],
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
    pub fn new(layouts: Vec<Layout>) -> Self {
        Self {
            layouts,
            ..Self::default()
        }
    }

    /// Create with every layout this build ships, in catalogue order.
    ///
    /// The list a layout picker offers, and — until there is a way to install
    /// one — the widest list there is.
    pub fn with_builtins() -> Self {
        Self::new(keylayout::builtins().to_vec())
    }

    /// Create with the named layouts, in the order named.
    ///
    /// Names this build does not know are skipped rather than refused: a
    /// settings file written by a later build must still leave the user with a
    /// keyboard. If *none* of them are known the manager falls back to the
    /// default layout, which is the same rule the compositor applies.
    pub fn with_ids(ids: &[&str]) -> Self {
        let layouts: Vec<Layout> = ids
            .iter()
            .filter_map(|id| keylayout::by_id(id))
            .cloned()
            .collect();
        if layouts.is_empty() {
            Self::default()
        } else {
            Self::new(layouts)
        }
    }

    /// Get the active layout.
    pub fn active_layout(&self) -> Option<&Layout> {
        self.layouts.get(self.active_index)
    }

    /// Get the short label for the tray indicator.
    pub fn tray_label(&self) -> &str {
        self.active_layout().map_or("??", |l| l.short_label)
    }

    /// The id of the active layout, as `inputsettings` spells it.
    ///
    /// This is the whole output of the selector: what the shell writes to
    /// `keyboard.layout` so the compositor starts translating with it.
    pub fn active_layout_id(&self) -> Option<&'static str> {
        self.active_layout().map(|l| l.id)
    }

    /// Cycle to the next layout.
    pub fn next_layout(&mut self) {
        self.active_index = step::wrapping_after(self.layouts.len(), self.active_index);
    }

    /// Cycle to the previous layout.
    pub fn prev_layout(&mut self) {
        self.active_index = step::wrapping_before(self.layouts.len(), self.active_index);
    }

    /// Switch to a specific layout by id.
    pub fn switch_to(&mut self, id: &str) -> bool {
        if let Some(idx) = self.layouts.iter().position(|l| l.id == id) {
            self.active_index = idx;
            true
        } else {
            false
        }
    }

    /// Add a layout (if not already installed).
    pub fn add_layout(&mut self, layout: Layout) -> bool {
        if self.layouts.iter().any(|l| l.id == layout.id) {
            return false;
        }
        self.layouts.push(layout);
        true
    }

    /// Add the built-in layout with this id, if this build has one.
    pub fn add_builtin(&mut self, id: &str) -> bool {
        keylayout::by_id(id).is_some_and(|l| self.add_layout(l.clone()))
    }

    /// Remove a layout by id. Cannot remove the last layout.
    pub fn remove_layout(&mut self, id: &str) -> bool {
        if self.layouts.len() <= 1 {
            return false;
        }
        if let Some(idx) = self.layouts.iter().position(|l| l.id == id) {
            self.layouts.remove(idx);
            // Clamp rather than subtract: the `len() <= 1` guard above is four
            // statements and one `remove` away from this line, which is where
            // the proof that the list is still non-empty would have to come
            // from.
            self.active_index = self.active_index.min(self.layouts.len().saturating_sub(1));
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
            && idx < self.layouts.len()
        {
            self.active_index = idx;
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
    pub fn render_tray_indicator(&self, p: &Palette, x: f32, y: f32) -> Vec<RenderCommand> {
        let label = self.tray_label();
        let chip = p.surface0;
        let chip_ink = p.text;

        vec![
            RenderCommand::FillRect {
                x,
                y,
                width: 28.0,
                height: 20.0,
                color: chip,
                corner_radii: CornerRadii::all(4.0),
            },
            RenderCommand::Text {
                x: x + 4.0,
                y: y + 3.0,
                text: label.to_string(),
                font_size: 11.0,
                color: chip_ink,
                font_weight: FontWeightHint::Bold,
                max_width: Some(24.0),
                overflow: TextOverflow::Ellipsis,
            },
        ]
    }

    /// Render the keyboard layout preview popup.
    pub fn render_preview(
        &self,
        p: &Palette,
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

        let popup_fill = p.base;
        let popup_edge = p.surface1;
        let title_ink = p.text;
        let cap = p.surface0;
        let cap_ink = p.text;

        let mut cmds = Vec::with_capacity(80);
        let height = 200.0;

        // Background
        cmds.push(RenderCommand::FillRect {
            x: popup_x,
            y: popup_y,
            width,
            height,
            color: popup_fill,
            corner_radii: CornerRadii::all(8.0),
        });

        // Border
        cmds.push(RenderCommand::StrokeRect {
            x: popup_x,
            y: popup_y,
            width,
            height,
            color: popup_edge,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: popup_x + 12.0,
            y: popup_y + 8.0,
            text: layout.display_name.to_string(),
            font_size: 13.0,
            color: title_ink,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Render keyboard rows
        let key_size = KEY_SIZE;
        let key_gap = 3.0;
        let row_offsets = [0.0_f32, 12.0, 20.0, 32.0]; // Stagger offsets
        let start_y = popup_y + 32.0;

        // The rows come from the same table the compositor types with, so a
        // cap can only ever be drawn with the character that key produces.
        // Unshifted: the preview answers "where is the letter", and a board
        // photographed with every cap shifted is not what anyone looks at.
        for row_idx in 0..4 {
            let row_y = start_y + row_idx as f32 * (key_size + key_gap);
            let stagger = row_offsets.get(row_idx).copied().unwrap_or(0.0);

            for (col_idx, def) in layout.row(row_idx).iter().enumerate() {
                let ch = def.plain;
                let key_x = popup_x + 8.0 + stagger + col_idx as f32 * (key_size + key_gap);

                // Key background
                cmds.push(RenderCommand::FillRect {
                    x: key_x,
                    y: row_y,
                    width: key_size,
                    height: key_size,
                    color: cap,
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
                    color: cap_ink,
                    font_weight: FontWeightHint::Regular,
                    // The label starts 6px in, so it gets `key_size` less
                    // *both* insets. It used to get `key_size - 4.0`, which
                    // let a wide glyph paint 2px onto the neighbouring cap.
                    max_width: Some(key_size - 12.0),
                    overflow: TextOverflow::Ellipsis,
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
            out.push_str(&format!("layout_{}={}\n", i, layout.id));
        }
        out.push_str(&format!("active={}\n", self.active_index));

        out
    }

    /// Parse config from text.
    ///
    /// The `layout_N=` lines used to be ignored — the doc comment said
    /// "layouts must be resolved separately" — because there was no catalogue
    /// to resolve a name against, so the caller had to hand the manager a list
    /// and hope it matched the file it was about to parse. [`keylayout`] is
    /// that catalogue, and the file's own list is now honoured.
    ///
    /// Two rules keep a bad file from costing the user their keyboard:
    ///
    /// * A name is resolved against the **installed** layouts first and the
    ///   catalogue second, so a layout the user wrote themselves survives a
    ///   round trip through the file even though the catalogue has never
    ///   heard of it.
    /// * If the file names no layouts at all, or none that resolve, the
    ///   installed list is left alone rather than emptied.
    ///
    /// `active` is applied against the list the file asked for, not the one
    /// that was installed when parsing began — otherwise an index would be
    /// checked against the wrong list and the file's own last line could
    /// select nothing.
    pub fn apply_config_text(&mut self, text: &str) {
        let mut named: Vec<(usize, String)> = Vec::new();
        let mut active: Option<usize> = None;

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, val)) = line.split_once('=') else {
                continue;
            };
            let (key, val) = (key.trim(), val.trim());
            match key {
                "switch_shortcut" => self.switch_shortcut = SwitchShortcut::from_id(val),
                "per_app_layout" => self.per_app_layout = val == "true",
                "active" => active = val.parse::<usize>().ok(),
                _ => {
                    if let Some(n) = key.strip_prefix("layout_")
                        && let Ok(position) = n.parse::<usize>()
                    {
                        named.push((position, val.to_string()));
                    }
                }
            }
        }

        // By declared position, so a file whose lines were reordered still
        // installs the layouts in the order it names them.
        named.sort_by_key(|(position, _)| *position);
        let resolved: Vec<Layout> = named
            .iter()
            .filter_map(|(_, id)| {
                self.layouts
                    .iter()
                    .find(|l| l.id == id.as_str())
                    .or_else(|| keylayout::by_id(id))
                    .cloned()
            })
            .collect();
        if !resolved.is_empty() {
            self.layouts = resolved;
            self.active_index = self.active_index.min(self.layouts.len().saturating_sub(1));
        }

        if let Some(idx) = active
            && idx < self.layouts.len()
        {
            self.active_index = idx;
        }
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
    use guitk::color::Color;

    // ---- Colour fixtures and helpers ----

    fn dark() -> Palette {
        Palette::for_mode(false)
    }

    /// A palette whose accent is a colour no role holds, so "this site wears the
    /// accent" and "this site wears blue" are distinguishable. Stock accent *is*
    /// blue, which is the whole reason this module's title was wrong.
    fn accented(light: bool) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = Color::from_hex(0x00FF_00FF);
        p
    }

    /// Four keys in known positions, so the expected command vector can be
    /// written out by hand instead of derived from the same rows the renderer
    /// reads — an expectation computed from the code under test asserts nothing.
    /// The empty third row also exercises a row that contributes no keys.
    fn tiny_layout() -> Layout {
        keylayout::LayoutSpec {
            id: "tiny",
            display_name: "Tiny",
            short_label: "TY",
            language: "xx",
            plain: ["ab", "c", "", "d"],
            shifted: ["AB", "C", "", "D"],
            iso_extra: None,
            altgr: &[],
        }
        .build()
    }

    /// A built-in, by the name the settings file uses for it.
    fn builtin(id: &str) -> Layout {
        keylayout::by_id(id)
            .unwrap_or_else(|| panic!("this build ships no layout called {id}"))
            .clone()
    }

    fn previewing(layout: Layout) -> InputMethodManager {
        let mut mgr = InputMethodManager::new(vec![layout]);
        mgr.preview_visible = true;
        mgr
    }

    /// Every colour the commands carry, in draw order.
    fn colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Text { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// WCAG relative-luminance contrast ratio.
    fn contrast(a: Color, b: Color) -> f64 {
        fn lum(c: Color) -> f64 {
            fn chan(v: u8) -> f64 {
                let s = f64::from(v) / 255.0;
                if s <= 0.039_28 {
                    s / 12.92
                } else {
                    ((s + 0.055) / 1.055).powf(2.4)
                }
            }
            0.2126 * chan(c.r) + 0.7152 * chan(c.g) + 0.0722 * chan(c.b)
        }
        let (x, y) = (lum(a), lum(b));
        let (hi, lo) = if x > y { (x, y) } else { (y, x) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// The tray's sites, in draw order.
    fn expected_tray(p: &Palette) -> Vec<(&'static str, Color)> {
        vec![("the tray chip", p.surface0), ("the tray label", p.text)]
    }

    /// The preview's sites, in draw order, for [`tiny_layout`]'s four keys.
    fn expected_preview(p: &Palette) -> Vec<(&'static str, Color)> {
        vec![
            ("the popup's fill", p.base),
            ("the popup's edge", p.surface1),
            ("the layout name", p.text),
            ("key 1's cap", p.surface0),
            ("key 1's label", p.text),
            ("key 2's cap", p.surface0),
            ("key 2's label", p.text),
            ("key 3's cap", p.surface0),
            ("key 3's label", p.text),
            ("key 4's cap", p.surface0),
            ("key 4's label", p.text),
        ]
    }

    // ---- The catalogue this module selects from ----

    /// The refactor's own regression test.
    ///
    /// This module used to hold its own copy of every layout table, and the
    /// copy is what the preview drew. So the preview could — and did — show a
    /// keyboard nobody could type on: picking Dvorak here changed the picture
    /// and left the compositor typing QWERTY. The guard is that the caps come
    /// out of `keylayout`, the same table the compositor translates with, so
    /// there is no second copy left to disagree with.
    #[test]
    fn the_preview_draws_the_catalogues_own_characters() {
        let dvorak = builtin("dvorak");
        let caps: String = previewing(dvorak.clone())
            .render_preview(&dark(), 0.0, 0.0, 400.0)
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text, font_size, ..
                } if (*font_size - 12.0).abs() < f32::EPSILON => Some(text.clone()),
                _ => None,
            })
            .collect();
        let from_table: String = dvorak.keys().iter().map(|k| k.plain).collect();
        assert_eq!(
            caps, from_table,
            "the preview drew something other than the layout's own characters"
        );
        assert!(
            caps.contains("aoeuidhtns"),
            "Dvorak's home row should be legible in the caps: {caps}"
        );
    }

    #[test]
    fn every_builtin_can_be_selected_by_the_name_the_settings_file_uses() {
        // The one join between this selector and the compositor is the id
        // string in `inputsettings`' `keyboard.layout`. A layout that cannot
        // be named cannot be chosen.
        let mgr = InputMethodManager::with_builtins();
        assert!(mgr.layouts.len() >= 5, "the catalogue lost layouts");
        for l in &mgr.layouts {
            let mut one = InputMethodManager::with_builtins();
            assert!(one.switch_to(l.id), "cannot switch to {}", l.id);
            assert_eq!(one.active_layout_id(), Some(l.id));
        }
    }

    #[test]
    fn a_layout_this_build_does_not_know_still_leaves_a_keyboard() {
        let mgr = InputMethodManager::with_ids(&["klingon-plqaD"]);
        assert_eq!(mgr.layouts.len(), 1);
        assert_eq!(mgr.active_layout_id(), Some(keylayout::DEFAULT_ID));
    }

    #[test]
    fn named_layouts_are_installed_in_the_order_they_were_named() {
        let mgr = InputMethodManager::with_ids(&["dvorak", "nonsense", "us-qwerty"]);
        let ids: Vec<&str> = mgr.layouts.iter().map(|l| l.id).collect();
        assert_eq!(
            ids,
            vec!["dvorak", "us-qwerty"],
            "the unknown one is skipped"
        );
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
        assert_eq!(SwitchShortcut::from_id("unknown"), SwitchShortcut::AltShift);
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
        let mut mgr = InputMethodManager::with_ids(&["us-qwerty", "dvorak"]);
        assert_eq!(mgr.tray_label(), "EN");
        mgr.next_layout();
        assert_eq!(mgr.tray_label(), "DV");
        mgr.next_layout();
        assert_eq!(mgr.tray_label(), "EN"); // wraps
    }

    #[test]
    fn test_manager_prev_layout() {
        let mut mgr = InputMethodManager::with_ids(&["us-qwerty", "dvorak", "colemak"]);
        mgr.prev_layout();
        assert_eq!(mgr.tray_label(), "CO"); // wraps to last
    }

    #[test]
    fn test_manager_switch_to() {
        let mut mgr = InputMethodManager::with_ids(&["us-qwerty", "dvorak"]);
        assert!(mgr.switch_to("dvorak"));
        assert_eq!(mgr.tray_label(), "DV");
    }

    #[test]
    fn test_manager_switch_to_nonexistent() {
        let mut mgr = InputMethodManager::default();
        assert!(!mgr.switch_to("nonexistent"));
    }

    #[test]
    fn test_manager_add_layout() {
        let mut mgr = InputMethodManager::default();
        assert!(mgr.add_builtin("dvorak"));
        assert_eq!(mgr.layouts.len(), 2);
    }

    #[test]
    fn test_manager_add_duplicate_fails() {
        let mut mgr = InputMethodManager::default();
        assert!(!mgr.add_builtin("us-qwerty"));
    }

    #[test]
    fn test_manager_remove_layout() {
        let mut mgr = InputMethodManager::with_ids(&["us-qwerty", "dvorak"]);
        assert!(mgr.remove_layout("dvorak"));
        assert_eq!(mgr.layouts.len(), 1);
    }

    #[test]
    fn test_manager_remove_last_fails() {
        let mut mgr = InputMethodManager::default();
        assert!(!mgr.remove_layout("us-qwerty"));
    }

    #[test]
    fn test_manager_remove_adjusts_active_index() {
        let mut mgr = InputMethodManager::with_ids(&["us-qwerty", "dvorak"]);
        mgr.active_index = 1;
        mgr.remove_layout("dvorak");
        assert_eq!(mgr.active_index, 0);
    }

    #[test]
    fn test_manager_per_app_layout() {
        let mut mgr = InputMethodManager::with_ids(&["us-qwerty", "dvorak"]);
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
        let mut mgr = InputMethodManager::with_ids(&["us-qwerty", "dvorak"]);
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
        let cmds = mgr.render_tray_indicator(&dark(), 100.0, 50.0);
        assert_eq!(cmds.len(), 2); // bg rect + text
    }

    #[test]
    fn test_manager_render_preview_hidden() {
        let mgr = InputMethodManager::default();
        let cmds = mgr.render_preview(&dark(), 0.0, 0.0, 400.0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_manager_render_preview_visible() {
        let mut mgr = InputMethodManager::default();
        mgr.preview_visible = true;
        let cmds = mgr.render_preview(&dark(), 0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_manager_config_roundtrip() {
        let mut mgr = InputMethodManager::with_ids(&["us-qwerty", "dvorak"]);
        mgr.switch_shortcut = SwitchShortcut::SuperSpace;
        mgr.per_app_layout = true;
        mgr.active_index = 1;

        let text = mgr.to_config_text();
        let mut mgr2 = InputMethodManager::with_ids(&["us-qwerty", "dvorak"]);
        mgr2.apply_config_text(&text);

        assert_eq!(mgr2.switch_shortcut, SwitchShortcut::SuperSpace);
        assert!(mgr2.per_app_layout);
        assert_eq!(mgr2.active_index, 1);
    }

    #[test]
    fn the_config_carries_which_layouts_are_installed_and_in_what_order() {
        // `to_config_text` always wrote the `layout_N=` lines and
        // `apply_config_text` always threw them away, so a shell restarted
        // from its own config came back with whatever list its caller happened
        // to hand it. There was no catalogue to resolve a name against; now
        // there is.
        let mut mgr = InputMethodManager::with_ids(&["de-qwertz", "dvorak", "us-qwerty"]);
        mgr.active_index = 1;

        let mut restored = InputMethodManager::default();
        restored.apply_config_text(&mgr.to_config_text());

        let ids: Vec<&str> = restored.layouts.iter().map(|l| l.id).collect();
        assert_eq!(ids, vec!["de-qwertz", "dvorak", "us-qwerty"]);
        assert_eq!(restored.active_layout_id(), Some("dvorak"));
    }

    #[test]
    fn a_layout_the_catalogue_never_heard_of_survives_a_round_trip() {
        // The rule that makes a user-written layout a first-class member of
        // the list: a name is looked up among the *installed* layouts before
        // the catalogue, so saving and reloading cannot quietly drop the one
        // layout that only exists on this machine.
        let mut mgr = InputMethodManager::new(vec![tiny_layout(), builtin("dvorak")]);
        mgr.active_index = 0;
        let text = mgr.to_config_text();

        let mut restored = InputMethodManager::new(vec![tiny_layout()]);
        restored.apply_config_text(&text);

        let ids: Vec<&str> = restored.layouts.iter().map(|l| l.id).collect();
        assert_eq!(ids, vec!["tiny", "dvorak"]);
    }

    #[test]
    fn a_config_that_names_no_layout_leaves_the_installed_list_alone() {
        // A truncated or half-written file must not be able to empty the list
        // — a manager with no layouts has no keyboard to show and no id to
        // hand the compositor.
        let mut mgr = InputMethodManager::with_ids(&["us-qwerty", "dvorak"]);
        mgr.apply_config_text("per_app_layout=true\nactive=1\n");
        assert_eq!(mgr.layouts.len(), 2);
        assert_eq!(mgr.active_layout_id(), Some("dvorak"));
    }

    #[test]
    fn an_active_index_past_the_end_of_the_file_s_own_list_is_refused() {
        // The ordering trap this parser had: `active` used to be range-checked
        // against the list installed *before* parsing, so a file listing one
        // layout and `active=4` could be accepted against a longer list that
        // was about to be replaced.
        let mut mgr = InputMethodManager::with_ids(&["us-qwerty", "dvorak", "colemak"]);
        mgr.apply_config_text("layout_0=dvorak\nactive=2\n");
        assert_eq!(mgr.layouts.len(), 1);
        assert_eq!(mgr.active_index, 0, "an out-of-range index selects nothing");
        assert_eq!(mgr.active_layout_id(), Some("dvorak"));
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

    // ---- Colour tests ----

    #[test]
    fn every_tray_site_draws_the_role_it_claims() {
        for light in [false, true] {
            let p = accented(light);
            let drawn = colors(&InputMethodManager::default().render_tray_indicator(&p, 0.0, 0.0));
            let want = expected_tray(&p);
            assert_eq!(
                drawn.len(),
                want.len(),
                "the tray drew {} colours, not {}",
                drawn.len(),
                want.len()
            );
            for ((what, expect), got) in want.into_iter().zip(&drawn) {
                assert_eq!(
                    format!("{got:?}"),
                    format!("{expect:?}"),
                    "{what} is wrong in {} mode",
                    if light { "light" } else { "dark" }
                );
            }
        }
    }

    #[test]
    fn every_preview_site_draws_the_role_it_claims() {
        for light in [false, true] {
            let p = accented(light);
            let drawn = colors(&previewing(tiny_layout()).render_preview(&p, 0.0, 0.0, 400.0));
            let want = expected_preview(&p);
            assert_eq!(
                drawn.len(),
                want.len(),
                "the preview drew {} colours, not the {} the table lists",
                drawn.len(),
                want.len()
            );
            for ((what, expect), got) in want.into_iter().zip(&drawn) {
                assert_eq!(
                    format!("{got:?}"),
                    format!("{expect:?}"),
                    "{what} is wrong in {} mode",
                    if light { "light" } else { "dark" }
                );
            }
        }
    }

    #[test]
    fn every_colour_the_tray_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = accented(light);
            crate::palette_check::assert_drawn_from(
                &p,
                &InputMethodManager::default().render_tray_indicator(&p, 0.0, 0.0),
                &[],
                "the input-method tray indicator",
            );
        }
    }

    #[test]
    fn every_colour_the_preview_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = accented(light);
            crate::palette_check::assert_drawn_from(
                &p,
                &previewing(builtin("de-qwertz")).render_preview(&p, 0.0, 0.0, 400.0),
                &[],
                "the keyboard layout preview",
            );
        }
    }

    #[test]
    fn every_site_the_tray_draws_moves_with_the_mode() {
        let mgr = InputMethodManager::default();
        let in_dark = colors(&mgr.render_tray_indicator(&Palette::for_mode(false), 0.0, 0.0));
        let in_light = colors(&mgr.render_tray_indicator(&Palette::for_mode(true), 0.0, 0.0));
        assert_eq!(in_dark.len(), in_light.len());
        // Pinned against the table, not merely against the other mode: the zip
        // below stops at the shorter side, so a site that stopped being drawn
        // would shrink both modes equally and never be asked whether it moves.
        assert_eq!(in_dark.len(), expected_tray(&dark()).len());
        for ((what, _), (d, l)) in expected_tray(&dark())
            .into_iter()
            .zip(in_dark.iter().zip(&in_light))
        {
            assert_ne!(
                format!("{d:?}"),
                format!("{l:?}"),
                "{what} draws the same colour in both modes"
            );
        }
    }

    #[test]
    fn every_site_the_preview_draws_moves_with_the_mode() {
        let mgr = previewing(tiny_layout());
        let in_dark = colors(&mgr.render_preview(&Palette::for_mode(false), 0.0, 0.0, 400.0));
        let in_light = colors(&mgr.render_preview(&Palette::for_mode(true), 0.0, 0.0, 400.0));
        assert_eq!(in_dark.len(), in_light.len());
        assert_eq!(in_dark.len(), expected_preview(&dark()).len());
        for ((what, _), (d, l)) in expected_preview(&dark())
            .into_iter()
            .zip(in_dark.iter().zip(&in_light))
        {
            assert_ne!(
                format!("{d:?}"),
                format!("{l:?}"),
                "{what} draws the same colour in both modes"
            );
        }
    }

    /// The module's headline decision, pinned as a negative.
    ///
    /// The accent means "you chose this", and nothing on either of these two
    /// surfaces has an unchosen sibling beside it to be chosen *over*. The
    /// preview's title used to be a hardcoded `#89B4FA` — which is precisely
    /// what the stock accent resolves to — so on a fresh install it looked like
    /// a deliberate accented heading and on any other theme it looked like a
    /// stray blue. This fails the moment any site reaches for `p.accent` again.
    #[test]
    fn no_site_in_this_module_wears_the_accent() {
        for light in [false, true] {
            let p = accented(light);
            let mut drawn =
                colors(&InputMethodManager::default().render_tray_indicator(&p, 0.0, 0.0));
            drawn.extend(colors(
                &previewing(tiny_layout()).render_preview(&p, 0.0, 0.0, 400.0),
            ));
            assert!(!drawn.is_empty());
            for c in drawn {
                assert_ne!(
                    format!("{c:?}"),
                    format!("{:?}", p.accent),
                    "a site wears the accent in {} mode",
                    if light { "light" } else { "dark" }
                );
            }
        }
    }

    /// Contrast, read out of the rendered commands rather than a table written
    /// by hand — see `design-decisions.md` §530 for why the table form catches
    /// nothing in its own module.
    ///
    /// The pairing rule is "an ink sits on the most recent fill", not "on the
    /// fill immediately before it": the popup draws its background, then its
    /// border, then the title, so an adjacency walk would skip the one pairing
    /// the popup chrome actually has. Tracking the last `FillRect` seen gets the
    /// caps right too, since each cap is the last fill before its own label.
    #[test]
    fn every_ink_this_module_draws_is_readable_on_what_it_sits_on() {
        let mut checked = 0usize;
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let mut scene = InputMethodManager::default().render_tray_indicator(&p, 0.0, 0.0);
            scene.extend(previewing(tiny_layout()).render_preview(&p, 0.0, 0.0, 400.0));

            let mut under: Option<Color> = None;
            for cmd in &scene {
                match cmd {
                    RenderCommand::FillRect { color, .. } => under = Some(*color),
                    RenderCommand::Text { color: ink, .. } => {
                        let Some(fill) = under else {
                            panic!("an ink is drawn before any fill");
                        };
                        let ratio = contrast(fill, *ink);
                        assert!(
                            ratio >= 4.5,
                            "{ink:?} on {fill:?} is {ratio:.2}:1 in {} mode",
                            if light { "light" } else { "dark" }
                        );
                        checked += 1;
                    }
                    _ => {}
                }
            }
        }
        // Tray label, popup title, four key labels; twice over for the modes.
        assert_eq!(checked, 12, "the pairing walk did not reach every ink");
    }

    /// Three fills that have to stay three: the popup, its hairline frame, and
    /// the key caps.
    ///
    /// Contrast has nothing to say about any of these pairings — none is an ink
    /// on a fill — so a defect that collapses two of them into one value is
    /// invisible to every legibility check in the module while being obvious on
    /// screen: caps at `surface1` become a grid of holes cut in the frame's own
    /// tone, and a border at `base` is a frame that isn't there. The module's
    /// header argues all three apart by construction, so this asserts the
    /// construction rather than its consequence.
    #[test]
    fn the_popup_its_border_and_its_key_caps_are_three_different_values() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let drawn = colors(&previewing(tiny_layout()).render_preview(&p, 0.0, 0.0, 400.0));
            let (fill, edge, cap) = (drawn[0], drawn[1], drawn[3]);
            assert_ne!(
                format!("{cap:?}"),
                format!("{fill:?}"),
                "the caps vanish into the popup"
            );
            assert_ne!(
                format!("{edge:?}"),
                format!("{fill:?}"),
                "the popup's border vanishes into the popup"
            );
            assert_ne!(
                format!("{cap:?}"),
                format!("{edge:?}"),
                "the caps match the popup's border"
            );
        }
    }

    /// A key label is clipped to a box that fits inside its own cap.
    ///
    /// It did not: the label started 6px in from a 28px cap and was given 24px
    /// of width, so a wide glyph could paint 2px past the cap's right edge and
    /// onto the neighbouring one. Nothing else in the module measures the caps,
    /// so without this the next widening would go unnoticed too.
    #[test]
    fn a_key_label_is_clipped_inside_its_own_cap() {
        let cmds = previewing(tiny_layout()).render_preview(&dark(), 0.0, 0.0, 400.0);
        let mut cap: Option<(f32, f32)> = None;
        let mut checked = 0usize;
        for cmd in &cmds {
            match cmd {
                RenderCommand::FillRect { x, width, .. }
                    if (*width - KEY_SIZE).abs() < f32::EPSILON =>
                {
                    cap = Some((*x, *width));
                }
                RenderCommand::Text { x, max_width, .. } => {
                    let Some((cap_x, cap_w)) = cap else { continue };
                    let Some(mw) = max_width else {
                        panic!("a key label is unbounded");
                    };
                    assert!(
                        *x + mw <= cap_x + cap_w,
                        "a key label may paint {:.1}px past its cap",
                        (*x + mw) - (cap_x + cap_w)
                    );
                    checked += 1;
                }
                _ => {}
            }
        }
        assert_eq!(checked, 4, "the walk did not reach every key label");
    }

    /// A branch nothing else renders: a manager with no layouts at all. The
    /// preview bails before drawing its background, so the popup is absent
    /// rather than empty-and-framed.
    #[test]
    fn a_manager_with_no_layouts_draws_no_preview() {
        let mgr = InputMethodManager {
            layouts: Vec::new(),
            active_index: 0,
            preview_visible: true,
            ..InputMethodManager::default()
        };
        assert!(mgr.render_preview(&dark(), 0.0, 0.0, 400.0).is_empty());
    }

    /// …but the tray still draws its chip, because the tray is how you find out
    /// something is wrong. It falls back to "??" rather than disappearing.
    #[test]
    fn a_manager_with_no_layouts_still_draws_a_tray_chip() {
        let mgr = InputMethodManager {
            layouts: Vec::new(),
            active_index: 0,
            ..InputMethodManager::default()
        };
        let cmds = mgr.render_tray_indicator(&dark(), 0.0, 0.0);
        assert_eq!(colors(&cmds), vec![dark().surface0, dark().text]);
        let RenderCommand::Text { text, .. } = &cmds[1] else {
            panic!("the tray's second command is not its label");
        };
        assert_eq!(text, "??");
    }
}
