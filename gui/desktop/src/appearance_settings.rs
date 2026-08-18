//! The appearance settings panel — the UI over the shared model.
//!
//! The values this edits, their configuration-file spellings and the file
//! itself all live in the `appearance` crate, because the Settings application
//! edits the same preferences and two copies of that model would let the two
//! processes disagree about what a user chose. What is left here is the panel:
//! tabs, rows, hit-testing, and the pending-vs-saved pair that decides whether
//! Save is live.

use appearance::{AccentColor, AppearanceFile, AppearanceSettings, ThemeMode, WindowCorners};
use appearance::{
    BASE, BLUE, CRUST, GREEN, LAVENDER, SUBTEXT0, SURFACE0, SURFACE1, SURFACE2, TEXT, YELLOW,
};
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use yamldoc::Document;

// ============================================================================
// UI: Appearance settings panel
// ============================================================================

/// Active tab in the appearance settings UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppearanceTab {
    /// Theme mode, accent color, transparency.
    Theme,
    /// Font settings.
    Fonts,
    /// Effects: animations, shadows, corners, taskbar style.
    Effects,
    /// Cursor and icon settings.
    CursorsIcons,
}

impl AppearanceTab {
    fn label(self) -> &'static str {
        match self {
            Self::Theme => "Theme",
            Self::Fonts => "Fonts",
            Self::Effects => "Effects",
            Self::CursorsIcons => "Cursors & Icons",
        }
    }
}

/// The settings group this panel persists — `appearance.yaml` in the user's
/// configuration directory.
///
/// Re-exported from the shared model, where the file's name belongs alongside
/// its schema.
pub use appearance::CONFIG_NAME;

/// Appearance settings UI state.
pub struct AppearanceSettingsUI {
    /// Active tab.
    pub active_tab: AppearanceTab,
    /// The settings being edited, together with the file they came from.
    file: AppearanceFile,
    /// Saved settings for revert/dirty detection.
    saved: AppearanceSettings,
}

impl AppearanceSettingsUI {
    pub fn new() -> Self {
        let file = AppearanceFile::new();
        Self {
            active_tab: AppearanceTab::Theme,
            saved: file.settings.clone(),
            file,
        }
    }

    /// Open the panel on the user's saved settings, reading
    /// `appearance.yaml`.
    ///
    /// A missing or unreadable file yields the defaults — the ordinary state
    /// on a fresh install, not an error to report to someone who has simply
    /// never changed a setting.
    #[must_use]
    pub fn load() -> Self {
        let file = AppearanceFile::load();
        Self {
            active_tab: AppearanceTab::Theme,
            saved: file.settings.clone(),
            file,
        }
    }

    /// Open the panel on an already-read configuration document. Split out
    /// from [`load`](Self::load) so the format can be tested without a
    /// filesystem.
    #[must_use]
    pub fn from_document(doc: Document) -> Self {
        let file = AppearanceFile::from_document(doc);
        Self {
            active_tab: AppearanceTab::Theme,
            saved: file.settings.clone(),
            file,
        }
    }

    /// The settings being edited.
    pub fn settings(&self) -> &AppearanceSettings {
        &self.file.settings
    }

    /// The settings being edited, for a control to change.
    pub fn settings_mut(&mut self) -> &mut AppearanceSettings {
        &mut self.file.settings
    }

    /// Whether settings have been changed from the saved state.
    ///
    /// A whole-struct comparison, not a hand-picked list of fields: the list
    /// this replaced omitted `mono_font`, `subpixel`, `smoothing` and
    /// `custom_accent`, so changing the terminal font left the Save button
    /// greyed out and the change was lost on close. A derived `PartialEq`
    /// cannot fall behind a new field the way a hand-written check does.
    pub fn is_dirty(&self) -> bool {
        self.file.settings != self.saved
    }

    /// Fold the current settings into the configuration document and mark
    /// them clean, without touching the filesystem.
    ///
    /// [`save`](Self::save) is the usual entry point; this exists so a caller
    /// that manages its own storage — or a test — can get the document.
    pub fn apply(&mut self) -> &Document {
        self.saved = self.file.settings.clone();
        self.file.apply()
    }

    /// Save the current settings to `appearance.yaml`.
    ///
    /// The settings are marked clean whether or not the write succeeds: the
    /// user's choices are in memory and in effect either way, and leaving the
    /// panel permanently dirty would only invite them to press Save again to
    /// the same result. The error is returned so the caller can say so.
    ///
    /// # Errors
    ///
    /// If there is no configuration directory, or the file cannot be written.
    pub fn save(&mut self) -> std::io::Result<()> {
        self.saved = self.file.settings.clone();
        self.file.save()
    }

    /// Revert to saved settings.
    pub fn revert(&mut self) {
        self.file.settings = self.saved.clone();
    }

    /// Switch tabs.
    pub fn set_tab(&mut self, tab: AppearanceTab) {
        self.active_tab = tab;
    }

    /// Render the appearance settings panel.
    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Panel background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: 24.0,
            y: 24.0,
            text: "Appearance".into(),
            font_size: 22.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 48.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Dirty indicator
        if self.is_dirty() {
            cmds.push(RenderCommand::FillRect {
                x: width - 100.0,
                y: 22.0,
                width: 76.0,
                height: 24.0,
                color: YELLOW,
                corner_radii: CornerRadii::all(12.0),
            });
            cmds.push(RenderCommand::Text {
                x: width - 92.0,
                y: 26.0,
                text: "Unsaved".into(),
                font_size: 12.0,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(64.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Tab bar
        let tabs = [
            AppearanceTab::Theme,
            AppearanceTab::Fonts,
            AppearanceTab::Effects,
            AppearanceTab::CursorsIcons,
        ];
        let tab_y = 60.0;

        let mut tx = 24.0;
        for &tab in &tabs {
            let active = tab == self.active_tab;
            let tab_w = text::padded_width_any_weight(tab.label(), 10.0, 13.0);

            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: tab_y,
                width: tab_w,
                height: 32.0,
                color: if active { BLUE } else { SURFACE0 },
                corner_radii: CornerRadii::all(6.0),
            });

            cmds.push(RenderCommand::Text {
                x: tx + 10.0,
                y: tab_y + 8.0,
                text: tab.label().into(),
                font_size: 13.0,
                color: if active { CRUST } else { SUBTEXT0 },
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_w - 20.0),
                overflow: TextOverflow::Ellipsis,
            });

            tx += tab_w + 8.0;
        }

        let content_y = tab_y + 48.0;
        let cw = width - 48.0;

        match self.active_tab {
            AppearanceTab::Theme => self.render_theme_tab(&mut cmds, 24.0, content_y, cw),
            AppearanceTab::Fonts => self.render_fonts_tab(&mut cmds, 24.0, content_y, cw),
            AppearanceTab::Effects => self.render_effects_tab(&mut cmds, 24.0, content_y, cw),
            AppearanceTab::CursorsIcons => self.render_cursors_tab(&mut cmds, 24.0, content_y, cw),
        }

        cmds
    }

    fn render_theme_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut cy = y;

        // Theme mode
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Theme Mode".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        for mode in ThemeMode::ALL {
            let selected = *mode == self.file.settings.theme_mode;
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width: 160.0,
                height: 30.0,
                color: if selected { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(6.0),
            });
            if selected {
                cmds.push(RenderCommand::StrokeRect {
                    x,
                    y: cy,
                    width: 160.0,
                    height: 30.0,
                    color: BLUE,
                    corner_radii: CornerRadii::all(6.0),
                    line_width: 2.0,
                });
            }
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: cy + 8.0,
                text: mode.label().into(),
                font_size: 13.0,
                color: if selected { TEXT } else { SUBTEXT0 },
                font_weight: FontWeightHint::Regular,
                max_width: Some(140.0),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 36.0;
        }

        cy += 8.0;

        // Accent color
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Accent Color".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        // Color swatches in a grid
        let swatch_size = 28.0;
        let swatch_gap = 8.0;
        let cols = 7;
        for (i, accent) in AccentColor::presets().iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let sx = x + (col as f32) * (swatch_size + swatch_gap);
            let sy = cy + (row as f32) * (swatch_size + swatch_gap);

            cmds.push(RenderCommand::FillRect {
                x: sx,
                y: sy,
                width: swatch_size,
                height: swatch_size,
                color: accent.color(),
                corner_radii: CornerRadii::all(swatch_size / 2.0),
            });

            if *accent == self.file.settings.accent_color {
                cmds.push(RenderCommand::StrokeRect {
                    x: sx - 3.0,
                    y: sy - 3.0,
                    width: swatch_size + 6.0,
                    height: swatch_size + 6.0,
                    color: TEXT,
                    corner_radii: CornerRadii::all((swatch_size + 6.0) / 2.0),
                    line_width: 2.0,
                });
            }
        }

        let rows = AccentColor::presets().len().div_ceil(cols);
        cy += (rows as f32) * (swatch_size + swatch_gap) + 16.0;

        // Current accent display
        let accent = self.file.settings.effective_accent();
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: format!(
                "Current: {} (#{:02X}{:02X}{:02X})",
                self.file.settings.accent_color.label(),
                accent.r,
                accent.g,
                accent.b,
            ),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 32.0;

        // Transparency level
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Transparency".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_label_value(
            cmds,
            x,
            cy,
            width,
            "Level",
            self.file.settings.transparency.label(),
        );
        cy += 28.0;

        // Transparency preview bar
        let alpha = self.file.settings.transparency.panel_alpha();
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: 40.0,
            color: Color::rgba(SURFACE0.r, SURFACE0.g, SURFACE0.b, alpha),
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: cy + 12.0,
            text: format!("Panel preview (alpha: {})", alpha),
            font_size: 13.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 52.0;

        // Scaling
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Display Scaling".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_label_value(
            cmds,
            x,
            cy,
            width,
            "Scale",
            &format!("{}%", self.file.settings.scaling_percent),
        );
        let _ = cy;
    }

    fn render_fonts_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut cy = y;
        let fonts = &self.file.settings.fonts;

        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "System Font".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_label_value(cmds, x, cy, width, "Family", &fonts.ui_font);
        cy += 28.0;
        self.render_label_value(
            cmds,
            x,
            cy,
            width,
            "Size",
            &format!("{:.0}pt", fonts.ui_size),
        );
        cy += 36.0;

        // Font preview
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: 48.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: cy + 8.0,
            text: "The quick brown fox jumps over the lazy dog".into(),
            font_size: fonts.ui_size,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: cy + 28.0,
            text: "ABCDEFGHIJKLM 0123456789".into(),
            font_size: fonts.ui_size,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 60.0;

        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Monospace Font".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_label_value(cmds, x, cy, width, "Family", &fonts.mono_font);
        cy += 28.0;
        self.render_label_value(
            cmds,
            x,
            cy,
            width,
            "Size",
            &format!("{:.0}pt", fonts.mono_size),
        );
        cy += 36.0;

        // Mono preview
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: 36.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: cy + 10.0,
            text: "fn main() { println!(\"Hello\"); }".into(),
            font_size: fonts.mono_size,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 48.0;

        // Rendering settings
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Rendering".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_toggle_row(cmds, x, cy, width, "Font Hinting", fonts.hinting);
        cy += 32.0;
        self.render_toggle_row(cmds, x, cy, width, "Font Smoothing", fonts.smoothing);
        cy += 32.0;
        self.render_label_value(cmds, x, cy, width, "Subpixel", fonts.subpixel.label());
        let _ = cy;
    }

    fn render_effects_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut cy = y;

        // Animations
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Animations".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_label_value(
            cmds,
            x,
            cy,
            width,
            "Speed",
            self.file.settings.animation_speed.label(),
        );
        cy += 28.0;
        self.render_label_value(
            cmds,
            x,
            cy,
            width,
            "Multiplier",
            &format!("{:.2}x", self.file.settings.animation_speed.multiplier()),
        );
        cy += 36.0;

        // Window corners
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Window Corners".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        let corner_styles = [
            WindowCorners::Square,
            WindowCorners::Subtle,
            WindowCorners::Rounded,
            WindowCorners::ExtraRounded,
        ];
        for style in &corner_styles {
            let selected = *style == self.file.settings.window_corners;
            let preview_w = 50.0;
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width: preview_w,
                height: 30.0,
                color: if selected { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(style.radius()),
            });
            if selected {
                cmds.push(RenderCommand::StrokeRect {
                    x,
                    y: cy,
                    width: preview_w,
                    height: 30.0,
                    color: BLUE,
                    corner_radii: CornerRadii::all(style.radius()),
                    line_width: 2.0,
                });
            }
            cmds.push(RenderCommand::Text {
                x: x + preview_w + 12.0,
                y: cy + 8.0,
                text: style.label().into(),
                font_size: 13.0,
                color: if selected { TEXT } else { SUBTEXT0 },
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - preview_w - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 38.0;
        }

        cy += 8.0;

        // Taskbar style
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Taskbar Style".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_label_value(
            cmds,
            x,
            cy,
            width,
            "Style",
            self.file.settings.taskbar_style.label(),
        );
        cy += 36.0;

        // Toggle switches
        self.render_toggle_row(
            cmds,
            x,
            cy,
            width,
            "Accent on Taskbar",
            self.file.settings.accent_taskbar,
        );
        cy += 32.0;
        self.render_toggle_row(
            cmds,
            x,
            cy,
            width,
            "Accent on Title Bars",
            self.file.settings.accent_titlebars,
        );
        cy += 32.0;
        self.render_toggle_row(
            cmds,
            x,
            cy,
            width,
            "Drop Shadows",
            self.file.settings.drop_shadows,
        );
        let _ = cy;
    }

    fn render_cursors_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut cy = y;

        // Cursor settings
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Cursor".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_label_value(
            cmds,
            x,
            cy,
            width,
            "Size",
            self.file.settings.cursor_size.label(),
        );
        cy += 28.0;
        self.render_label_value(
            cmds,
            x,
            cy,
            width,
            "Scheme",
            self.file.settings.cursor_scheme.label(),
        );
        cy += 36.0;

        // Cursor preview
        let cursor_px = self.file.settings.cursor_size.pixels() as f32;
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width: cursor_px + 20.0,
            height: cursor_px + 20.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        // Simple arrow cursor approximation
        cmds.push(RenderCommand::FillRect {
            x: x + 10.0,
            y: cy + 10.0,
            width: cursor_px * 0.4,
            height: cursor_px,
            color: TEXT,
            corner_radii: CornerRadii::ZERO,
        });
        cy += cursor_px + 32.0;

        // Icon settings
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Desktop Icons".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_label_value(
            cmds,
            x,
            cy,
            width,
            "Size",
            self.file.settings.icon_size.label(),
        );
        cy += 28.0;

        // Icon size preview
        let icon_px = self.file.settings.icon_size.pixels() as f32;
        let preview_items = ["Documents", "Downloads", "Pictures"];
        for (i, name) in preview_items.iter().enumerate() {
            let ix = x + (i as f32) * (icon_px + 24.0);
            cmds.push(RenderCommand::FillRect {
                x: ix,
                y: cy,
                width: icon_px,
                height: icon_px,
                color: SURFACE1,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: ix,
                y: cy + icon_px + 4.0,
                text: (*name).into(),
                font_size: 10.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(icon_px),
                overflow: TextOverflow::Ellipsis,
            });
        }
        let _ = cy;
    }

    // ---- Render helpers ----

    fn render_toggle_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        enabled: bool,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y: y + 4.0,
            text: label.into(),
            font_size: 14.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 80.0),
            overflow: TextOverflow::Ellipsis,
        });

        let sw_x = x + width - 44.0;
        cmds.push(RenderCommand::FillRect {
            x: sw_x,
            y: y + 2.0,
            width: 40.0,
            height: 22.0,
            color: if enabled { GREEN } else { SURFACE2 },
            corner_radii: CornerRadii::all(11.0),
        });
        let knob_x = if enabled { sw_x + 20.0 } else { sw_x + 2.0 };
        cmds.push(RenderCommand::FillRect {
            x: knob_x,
            y: y + 4.0,
            width: 18.0,
            height: 18.0,
            color: TEXT,
            corner_radii: CornerRadii::all(9.0),
        });
    }

    fn render_label_value(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        value: &str,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: label.into(),
            font_size: 13.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.55,
            y,
            text: value.into(),
            font_size: 13.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.45),
            overflow: TextOverflow::Ellipsis,
        });
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
    // Edited through the panel but named only by the tests, which set a
    // non-default value to check that the Save button notices.
    use appearance::{CursorScheme, SubpixelMode};

    // ---- AppearanceSettingsUI ----

    #[test]
    fn test_ui_new() {
        let ui = AppearanceSettingsUI::new();
        assert_eq!(ui.active_tab, AppearanceTab::Theme);
        assert!(!ui.is_dirty());
    }

    #[test]
    fn test_ui_dirty_detection() {
        let mut ui = AppearanceSettingsUI::new();
        assert!(!ui.is_dirty());
        ui.file.settings.theme_mode = ThemeMode::Light;
        assert!(ui.is_dirty());
    }

    #[test]
    fn test_ui_save() {
        let mut ui = AppearanceSettingsUI::new();
        ui.file.settings.accent_color = AccentColor::Red;
        assert!(ui.is_dirty());
        // `apply`, not `save`: the test is about the clean/dirty transition,
        // and `save` would write to the developer's own config directory.
        ui.apply();
        assert!(!ui.is_dirty());
    }

    #[test]
    fn test_ui_revert() {
        let mut ui = AppearanceSettingsUI::new();
        ui.file.settings.theme_mode = ThemeMode::Light;
        ui.file.settings.accent_color = AccentColor::Green;
        assert!(ui.is_dirty());
        ui.revert();
        assert!(!ui.is_dirty());
        assert_eq!(ui.file.settings.theme_mode, ThemeMode::Dark);
        assert_eq!(ui.file.settings.accent_color, AccentColor::Blue);
    }

    #[test]
    fn test_ui_set_tab() {
        let mut ui = AppearanceSettingsUI::new();
        ui.set_tab(AppearanceTab::Fonts);
        assert_eq!(ui.active_tab, AppearanceTab::Fonts);
    }

    #[test]
    fn test_ui_render_theme_tab() {
        let ui = AppearanceSettingsUI::new();
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_fonts_tab() {
        let mut ui = AppearanceSettingsUI::new();
        ui.set_tab(AppearanceTab::Fonts);
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_effects_tab() {
        let mut ui = AppearanceSettingsUI::new();
        ui.set_tab(AppearanceTab::Effects);
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_cursors_tab() {
        let mut ui = AppearanceSettingsUI::new();
        ui.set_tab(AppearanceTab::CursorsIcons);
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_with_dirty() {
        let mut ui = AppearanceSettingsUI::new();
        ui.file.settings.theme_mode = ThemeMode::Light;
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_dirty_font_change() {
        let mut ui = AppearanceSettingsUI::new();
        ui.file.settings.fonts.ui_size = 16.0;
        assert!(ui.is_dirty());
    }

    #[test]
    fn test_ui_dirty_cursor_change() {
        let mut ui = AppearanceSettingsUI::new();
        ui.file.settings.cursor_scheme = CursorScheme::Inverted;
        assert!(ui.is_dirty());
    }

    // ---- Tab labels ----

    #[test]
    fn test_tab_labels() {
        assert_eq!(AppearanceTab::Theme.label(), "Theme");
        assert_eq!(AppearanceTab::Fonts.label(), "Fonts");
        assert_eq!(AppearanceTab::Effects.label(), "Effects");
        assert_eq!(AppearanceTab::CursorsIcons.label(), "Cursors & Icons");
    }

    #[test]
    fn test_config_save_preserves_the_users_comments_and_keys() {
        // The whole point of yamldoc: a user annotates their file, changes one
        // setting in the UI, and gets their annotations back.
        let original = "\
# My desktop. Do not let the settings app eat these notes.
theme:
  mode: dark      # I like it dark
  accent: teal

# Something a future version of the desktop writes.
experimental:
  wobbly_windows: true
";
        let mut ui = AppearanceSettingsUI::from_document(Document::parse(original));
        assert_eq!(ui.file.settings.theme_mode, ThemeMode::Dark);
        assert_eq!(ui.file.settings.accent_color, AccentColor::Teal);

        ui.file.settings.accent_color = AccentColor::Mauve;
        let text = ui.apply().to_text();

        assert!(text.contains("# My desktop. Do not let the settings app eat these notes."));
        assert!(text.contains("# I like it dark"));
        assert!(
            text.contains("wobbly_windows: true"),
            "a key this version does not model was deleted:\n{text}"
        );
        assert!(
            text.contains("accent: mauve"),
            "the edit did not land:\n{text}"
        );
        assert!(!text.contains("accent: teal"));
        assert_eq!(
            AppearanceSettings::read_from(&Document::parse(&text)).accent_color,
            AccentColor::Mauve
        );
    }

    #[test]
    fn test_config_saving_twice_produces_no_second_diff() {
        // Otherwise every visit to the settings panel dirties the user's file.
        let mut ui =
            AppearanceSettingsUI::from_document(Document::parse("# notes\ntheme:\n  mode: dark\n"));
        ui.file.settings.fonts.ui_size = 14.0;
        let once = ui.apply().to_text();
        let twice = ui.apply().to_text();
        assert_eq!(once, twice);
        // And a save that changes nothing does not rewrite the file either.
        let mut again = AppearanceSettingsUI::from_document(Document::parse(&once));
        assert_eq!(again.apply().to_text(), once);
    }

    #[test]
    fn test_ui_dirty_detects_the_fields_the_old_check_missed() {
        // Each of these left Save greyed out before `is_dirty` compared the
        // whole struct, so the change was silently lost on close.
        let mut ui = AppearanceSettingsUI::new();
        ui.file.settings.fonts.mono_font = "Iosevka".to_string();
        assert!(ui.is_dirty(), "mono_font change not detected");

        let mut ui = AppearanceSettingsUI::new();
        ui.file.settings.fonts.subpixel = SubpixelMode::Bgr;
        assert!(ui.is_dirty(), "subpixel change not detected");

        let mut ui = AppearanceSettingsUI::new();
        ui.file.settings.fonts.smoothing = !ui.file.settings.fonts.smoothing;
        assert!(ui.is_dirty(), "smoothing change not detected");

        let mut ui = AppearanceSettingsUI::new();
        ui.file.settings.custom_accent = Color::rgb(1, 2, 3);
        assert!(ui.is_dirty(), "custom_accent change not detected");
    }

    #[test]
    fn test_ui_dirty_detects_a_font_size_nudge() {
        // The old check ignored size changes under 0.1pt, which meant a slider
        // step could be a change the user could see but not save.
        let mut ui = AppearanceSettingsUI::new();
        ui.file.settings.fonts.ui_size += 0.05;
        assert!(ui.is_dirty());
        ui.apply();
        assert!(!ui.is_dirty());
    }

    #[test]
    fn test_ui_load_defaults_from_an_empty_document() {
        let ui = AppearanceSettingsUI::from_document(Document::new());
        assert_eq!(ui.file.settings, AppearanceSettings::default());
        assert!(!ui.is_dirty());
    }
}
