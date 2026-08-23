//! Accessibility settings panel for the desktop shell.
//!
//! Provides configuration for visual accessibility (high contrast,
//! color filters, text scaling), input assistance (sticky keys, filter
//! keys, mouse keys), and audio accessibility (visual alerts, mono audio,
//! captions). Extends the core a11y module with a full settings UI.
//!
//! # Colour
//!
//! Every colour here is read from the [`Palette`] the caller supplies. The
//! nine Mocha constants this module used to hold are gone, which is what
//! makes it follow the light/dark switch and the accent at all. Three of the
//! substitutions were judgements rather than lookups, and they are worth
//! stating because a later reader would otherwise reasonably "correct" them
//! back.
//!
//! 1. **The selected tab's lettering is no longer near-black.** It was
//!    `CRUST` (`#11111B`) drawn on `BLUE`, which in Mocha measures 8.91:1 —
//!    excellent. But the same pair in Latte is near-black on `#1D62EC`, which
//!    measures **3.58:1** and fails the 4.5:1 floor. A constant that is
//!    correct in the palette it was written for and wrong in the other one is
//!    exactly the defect this conversion exists to remove, and it is least
//!    forgivable *here*, on the page a low-vision user opens first. The ink
//!    is now [`Palette::on_accent`], which reads the fill's brightness and
//!    answers near-black or near-white accordingly.
//!
//! 2. **Green means "on"; the accent means "chosen".** The deleted `GREEN`
//!    appeared at two sites that look alike and are not: the "*n* features
//!    active" line and the toggle switch's pill both encode **state**, a code
//!    the user decodes, so they stay `p.green` and do not follow the accent.
//!    The tab pill's `BLUE` encoded **selection**, which is what the accent
//!    is for, so it becomes `p.accent`. Had both been rewritten the same way,
//!    a user with a green accent would find every switch reading "on".
//!
//! 3. **The section headings keep their hue.** `p.lavender` at 15pt Bold is
//!    the heading convention across the settings pages, and it survives the
//!    mode change on the numbers: 9.17:1 on Mocha `base`, 4.65:1 on Latte
//!    `base`. Both clear the floor, so there was no reason to demote them to
//!    `p.text`.

use appearance::Palette;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;

// ============================================================================
// Visual settings
// ============================================================================

/// High contrast mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContrastMode {
    Off,
    HighContrast,
    HighContrastInverse,
    IncreasedContrast,
}

impl ContrastMode {
    fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::HighContrast => "High Contrast",
            Self::HighContrastInverse => "Inverse",
            Self::IncreasedContrast => "Increased",
        }
    }
}

/// Color vision filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorFilter {
    Off,
    Grayscale,
    Deuteranopia,
    Protanopia,
    Tritanopia,
}

impl ColorFilter {
    fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Grayscale => "Grayscale",
            Self::Deuteranopia => "Red-Green (Deuteranopia)",
            Self::Protanopia => "Red-Green (Protanopia)",
            Self::Tritanopia => "Blue-Yellow (Tritanopia)",
        }
    }
}

/// Text size scaling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextScale {
    Normal,  // 100%
    Large,   // 125%
    Larger,  // 150%
    Largest, // 200%
}

impl TextScale {
    fn label(self) -> &'static str {
        match self {
            Self::Normal => "100%",
            Self::Large => "125%",
            Self::Larger => "150%",
            Self::Largest => "200%",
        }
    }

    pub fn factor(self) -> f32 {
        match self {
            Self::Normal => 1.0,
            Self::Large => 1.25,
            Self::Larger => 1.5,
            Self::Largest => 2.0,
        }
    }
}

/// Cursor appearance settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorIndicator {
    Off,
    Ring,
    Highlight,
    Trail,
}

impl CursorIndicator {
    fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Ring => "Ring",
            Self::Highlight => "Highlight",
            Self::Trail => "Trail",
        }
    }
}

/// Visual accessibility settings.
#[derive(Clone, Debug)]
pub struct VisualSettings {
    pub contrast_mode: ContrastMode,
    pub color_filter: ColorFilter,
    pub text_scale: TextScale,
    pub cursor_indicator: CursorIndicator,
    pub cursor_size_multiplier: f32,
    pub reduce_motion: bool,
    pub reduce_transparency: bool,
    pub always_show_scrollbars: bool,
    pub focus_indicator_width: u8,
    pub text_cursor_thickness: u8,
}

impl Default for VisualSettings {
    fn default() -> Self {
        Self {
            contrast_mode: ContrastMode::Off,
            color_filter: ColorFilter::Off,
            text_scale: TextScale::Normal,
            cursor_indicator: CursorIndicator::Off,
            cursor_size_multiplier: 1.0,
            reduce_motion: false,
            reduce_transparency: false,
            always_show_scrollbars: false,
            focus_indicator_width: 2,
            text_cursor_thickness: 1,
        }
    }
}

// ============================================================================
// Input assistance
// ============================================================================

/// Sticky keys mode — hold modifier keys without holding them physically.
#[derive(Clone, Debug)]
pub struct StickyKeysConfig {
    pub enabled: bool,
    pub lock_on_double_press: bool,
    pub release_on_two_keys: bool,
    pub play_sound: bool,
    pub show_indicator: bool,
}

impl Default for StickyKeysConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            lock_on_double_press: true,
            release_on_two_keys: true,
            play_sound: true,
            show_indicator: true,
        }
    }
}

/// Filter keys mode — ignore brief or repeated keystrokes.
#[derive(Clone, Debug)]
pub struct FilterKeysConfig {
    pub enabled: bool,
    pub acceptance_delay_ms: u32,
    pub repeat_delay_ms: u32,
    pub bounce_delay_ms: u32,
    pub play_sound: bool,
    pub show_indicator: bool,
}

impl Default for FilterKeysConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            acceptance_delay_ms: 300,
            repeat_delay_ms: 500,
            bounce_delay_ms: 200,
            play_sound: true,
            show_indicator: true,
        }
    }
}

/// Mouse keys mode — control pointer via keyboard numpad.
#[derive(Clone, Debug)]
pub struct MouseKeysConfig {
    pub enabled: bool,
    pub speed: u8,
    pub acceleration: bool,
    pub use_numpad: bool,
}

impl Default for MouseKeysConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            speed: 5,
            acceleration: true,
            use_numpad: true,
        }
    }
}

/// Input assistance settings aggregate.
#[derive(Clone, Debug)]
pub struct InputSettings {
    pub sticky_keys: StickyKeysConfig,
    pub filter_keys: FilterKeysConfig,
    pub mouse_keys: MouseKeysConfig,
    pub on_screen_keyboard: bool,
    pub auto_click: bool,
    pub auto_click_delay_ms: u32,
}

impl Default for InputSettings {
    fn default() -> Self {
        Self {
            sticky_keys: StickyKeysConfig::default(),
            filter_keys: FilterKeysConfig::default(),
            mouse_keys: MouseKeysConfig::default(),
            on_screen_keyboard: false,
            auto_click: false,
            auto_click_delay_ms: 1000,
        }
    }
}

// ============================================================================
// Audio accessibility
// ============================================================================

/// Audio accessibility settings.
#[derive(Clone, Debug)]
pub struct AudioA11ySettings {
    pub visual_alerts: bool,
    pub flash_screen: bool,
    pub mono_audio: bool,
    pub show_captions: bool,
    pub caption_font_size: f32,
    pub caption_background: bool,
}

impl Default for AudioA11ySettings {
    fn default() -> Self {
        Self {
            visual_alerts: false,
            flash_screen: false,
            mono_audio: false,
            show_captions: false,
            caption_font_size: 16.0,
            caption_background: true,
        }
    }
}

// ============================================================================
// Screen reader
// ============================================================================

/// Screen reader settings.
#[derive(Clone, Debug)]
pub struct ScreenReaderConfig {
    pub enabled: bool,
    pub speech_rate: u8,
    pub pitch: u8,
    pub volume: u8,
    pub verbosity: ScreenReaderVerbosity,
    pub read_typed_chars: bool,
    pub read_typed_words: bool,
    pub announce_notifications: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenReaderVerbosity {
    Low,
    Medium,
    High,
}

impl ScreenReaderVerbosity {
    fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }
}

impl Default for ScreenReaderConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            speech_rate: 50,
            pitch: 50,
            volume: 80,
            verbosity: ScreenReaderVerbosity::Medium,
            read_typed_chars: false,
            read_typed_words: true,
            announce_notifications: true,
        }
    }
}

// ============================================================================
// Magnifier
// ============================================================================

/// Screen magnifier settings.
#[derive(Clone, Debug)]
pub struct MagnifierConfig {
    pub enabled: bool,
    pub zoom_level: f32,
    pub lens_size: u32,
    pub follow_cursor: bool,
    pub follow_focus: bool,
    pub smooth_scrolling: bool,
    pub invert_colors: bool,
}

impl Default for MagnifierConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            zoom_level: 2.0,
            lens_size: 300,
            follow_cursor: true,
            follow_focus: true,
            smooth_scrolling: true,
            invert_colors: false,
        }
    }
}

// ============================================================================
// All accessibility settings
// ============================================================================

/// All accessibility settings combined.
#[derive(Clone, Debug, Default)]
pub struct AccessibilitySettings {
    pub visual: VisualSettings,
    pub input: InputSettings,
    pub audio: AudioA11ySettings,
    pub screen_reader: ScreenReaderConfig,
    pub magnifier: MagnifierConfig,
}

/// A top-level accessibility feature: one the user switches on or off as a
/// whole, as opposed to a setting that merely refines one of these (caption
/// font size refines captions; "follow cursor" refines the magnifier).
///
/// This enum exists because "the list of accessibility features" was
/// previously written out twice — once as fifteen `if … { count += 1 }` arms
/// in `active_feature_count`, and once as the toggle rows the settings pane
/// renders — with nothing tying the two together. They had already drifted:
/// `flash_screen` has a toggle row and is a peer of `visual_alerts` and
/// `mono_audio`, both of which were counted, but it was not. A user who
/// switched on Flash Screen and nothing else was told "0 features active",
/// which the pane renders by showing no banner at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A11yFeature {
    HighContrast,
    ColorFilter,
    ReduceMotion,
    ReduceTransparency,
    CursorIndicator,
    StickyKeys,
    FilterKeys,
    MouseKeys,
    OnScreenKeyboard,
    AutoClick,
    VisualAlerts,
    FlashScreen,
    MonoAudio,
    Captions,
    ScreenReader,
    Magnifier,
}

impl A11yFeature {
    /// Every feature, in the order the settings pane presents them.
    pub const ALL: [Self; 16] = [
        Self::HighContrast,
        Self::ColorFilter,
        Self::ReduceMotion,
        Self::ReduceTransparency,
        Self::CursorIndicator,
        Self::StickyKeys,
        Self::FilterKeys,
        Self::MouseKeys,
        Self::OnScreenKeyboard,
        Self::AutoClick,
        Self::VisualAlerts,
        Self::FlashScreen,
        Self::MonoAudio,
        Self::Captions,
        Self::ScreenReader,
        Self::Magnifier,
    ];

    /// Human-readable name, matching the settings pane's toggle labels.
    pub const fn label(self) -> &'static str {
        match self {
            Self::HighContrast => "High Contrast",
            Self::ColorFilter => "Color Filter",
            Self::ReduceMotion => "Reduce Motion",
            Self::ReduceTransparency => "Reduce Transparency",
            Self::CursorIndicator => "Cursor Indicator",
            Self::StickyKeys => "Sticky Keys",
            Self::FilterKeys => "Filter Keys",
            Self::MouseKeys => "Mouse Keys",
            Self::OnScreenKeyboard => "On-Screen Keyboard",
            Self::AutoClick => "Auto Click",
            Self::VisualAlerts => "Visual Alerts",
            Self::FlashScreen => "Flash Screen",
            Self::MonoAudio => "Mono Audio",
            Self::Captions => "Captions",
            Self::ScreenReader => "Screen Reader",
            Self::Magnifier => "Magnifier",
        }
    }
}

impl AccessibilitySettings {
    /// Whether one named feature is switched on.
    ///
    /// The `match` is exhaustive, so a new variant of [`A11yFeature`] cannot
    /// be added without deciding here what makes it active — which is the
    /// enforcement the fifteen parallel `if` arms did not have.
    pub const fn is_active(&self, feature: A11yFeature) -> bool {
        match feature {
            A11yFeature::HighContrast => !matches!(self.visual.contrast_mode, ContrastMode::Off),
            A11yFeature::ColorFilter => !matches!(self.visual.color_filter, ColorFilter::Off),
            A11yFeature::ReduceMotion => self.visual.reduce_motion,
            A11yFeature::ReduceTransparency => self.visual.reduce_transparency,
            A11yFeature::CursorIndicator => {
                !matches!(self.visual.cursor_indicator, CursorIndicator::Off)
            }
            A11yFeature::StickyKeys => self.input.sticky_keys.enabled,
            A11yFeature::FilterKeys => self.input.filter_keys.enabled,
            A11yFeature::MouseKeys => self.input.mouse_keys.enabled,
            A11yFeature::OnScreenKeyboard => self.input.on_screen_keyboard,
            A11yFeature::AutoClick => self.input.auto_click,
            A11yFeature::VisualAlerts => self.audio.visual_alerts,
            A11yFeature::FlashScreen => self.audio.flash_screen,
            A11yFeature::MonoAudio => self.audio.mono_audio,
            A11yFeature::Captions => self.audio.show_captions,
            A11yFeature::ScreenReader => self.screen_reader.enabled,
            A11yFeature::Magnifier => self.magnifier.enabled,
        }
    }

    /// Every feature currently switched on, in presentation order.
    pub fn active_features(&self) -> impl Iterator<Item = A11yFeature> + '_ {
        A11yFeature::ALL.into_iter().filter(|&f| self.is_active(f))
    }

    /// Count of active accessibility features.
    pub fn active_feature_count(&self) -> usize {
        self.active_features().count()
    }
}

// ============================================================================
// UI
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A11yTab {
    Visual,
    Input,
    Audio,
    Reader,
    Magnifier,
}

impl A11yTab {
    /// Every tab, in the order the bar draws them.
    ///
    /// Kept next to the variants rather than inline in `render` so a test can
    /// walk the same list the renderer walks — a test that hard-codes its own
    /// copy of the order stops testing the bar the moment a tab is added.
    pub const ALL: [Self; 5] = [
        Self::Visual,
        Self::Input,
        Self::Audio,
        Self::Reader,
        Self::Magnifier,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Visual => "Visual",
            Self::Input => "Input",
            Self::Audio => "Audio",
            Self::Reader => "Reader",
            Self::Magnifier => "Magnifier",
        }
    }
}

pub struct AccessibilitySettingsUI {
    pub active_tab: A11yTab,
    pub settings: AccessibilitySettings,
}

impl AccessibilitySettingsUI {
    pub fn new() -> Self {
        Self {
            active_tab: A11yTab::Visual,
            settings: AccessibilitySettings::default(),
        }
    }

    pub fn set_tab(&mut self, tab: A11yTab) {
        self.active_tab = tab;
    }

    pub fn render(&self, p: &Palette, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: p.base,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: 24.0,
            y: 24.0,
            text: "Accessibility".into(),
            font_size: 22.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 48.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Active features count. Green because it reports a *state* — see the
        // module's `# Colour` note; it deliberately does not follow the accent.
        let active = self.settings.active_feature_count();
        if active > 0 {
            cmds.push(RenderCommand::Text {
                x: 24.0,
                y: 50.0,
                text: format!("{} accessibility features active", active),
                font_size: 12.0,
                color: p.green,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 48.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Tabs
        let tab_y = 68.0;
        let mut tx = 24.0;
        for &tab in &A11yTab::ALL {
            let active_tab = tab == self.active_tab;
            let tw = text::padded_width_any_weight(tab.label(), 9.0, 12.0);
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: tab_y,
                width: tw,
                height: 30.0,
                color: if active_tab { p.accent } else { p.surface0 },
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 9.0,
                y: tab_y + 7.0,
                text: tab.label().into(),
                font_size: 12.0,
                // Read off the fill rather than fixed near-black: the old
                // `CRUST` measured 3.58:1 on the light accent.
                color: if active_tab {
                    p.on_accent()
                } else {
                    p.subtext0
                },
                font_weight: if active_tab {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tw - 18.0),
                overflow: TextOverflow::Ellipsis,
            });
            tx += tw + 6.0;
        }

        let cy = tab_y + 44.0;
        let cw = width - 48.0;

        match self.active_tab {
            A11yTab::Visual => self.render_visual(&mut cmds, p, 24.0, cy, cw),
            A11yTab::Input => self.render_input(&mut cmds, p, 24.0, cy, cw),
            A11yTab::Audio => self.render_audio(&mut cmds, p, 24.0, cy, cw),
            A11yTab::Reader => self.render_reader(&mut cmds, p, 24.0, cy, cw),
            A11yTab::Magnifier => self.render_magnifier(&mut cmds, p, 24.0, cy, cw),
        }

        cmds
    }

    fn render_visual(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut cy = y;
        let v = &self.settings.visual;

        self.render_label_value(cmds, p, x, cy, width, "Contrast", v.contrast_mode.label());
        cy += 28.0;
        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Color Filter",
            v.color_filter.label(),
        );
        cy += 28.0;
        self.render_label_value(cmds, p, x, cy, width, "Text Size", v.text_scale.label());
        cy += 28.0;
        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Cursor Indicator",
            v.cursor_indicator.label(),
        );
        cy += 28.0;
        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Cursor Size",
            &format!("{:.1}x", v.cursor_size_multiplier),
        );
        cy += 36.0;

        self.render_toggle_row(cmds, p, x, cy, width, "Reduce Motion", v.reduce_motion);
        cy += 32.0;
        self.render_toggle_row(
            cmds,
            p,
            x,
            cy,
            width,
            "Reduce Transparency",
            v.reduce_transparency,
        );
        cy += 32.0;
        self.render_toggle_row(
            cmds,
            p,
            x,
            cy,
            width,
            "Always Show Scrollbars",
            v.always_show_scrollbars,
        );
        cy += 36.0;

        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Focus Indicator",
            &format!("{}px", v.focus_indicator_width),
        );
        cy += 28.0;
        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Text Cursor",
            &format!("{}px", v.text_cursor_thickness),
        );
        let _ = cy;
    }

    fn render_input(&self, cmds: &mut Vec<RenderCommand>, p: &Palette, x: f32, y: f32, width: f32) {
        let mut cy = y;

        // Sticky keys
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Sticky Keys".into(),
            font_size: 15.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 24.0;
        self.render_toggle_row(
            cmds,
            p,
            x,
            cy,
            width,
            "Enable",
            self.settings.input.sticky_keys.enabled,
        );
        cy += 28.0;
        self.render_toggle_row(
            cmds,
            p,
            x,
            cy,
            width,
            "Lock on Double Press",
            self.settings.input.sticky_keys.lock_on_double_press,
        );
        cy += 28.0;
        self.render_toggle_row(
            cmds,
            p,
            x,
            cy,
            width,
            "Sound",
            self.settings.input.sticky_keys.play_sound,
        );
        cy += 36.0;

        // Filter keys
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Filter Keys".into(),
            font_size: 15.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 24.0;
        self.render_toggle_row(
            cmds,
            p,
            x,
            cy,
            width,
            "Enable",
            self.settings.input.filter_keys.enabled,
        );
        cy += 28.0;
        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Acceptance",
            &format!("{}ms", self.settings.input.filter_keys.acceptance_delay_ms),
        );
        cy += 28.0;
        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Bounce",
            &format!("{}ms", self.settings.input.filter_keys.bounce_delay_ms),
        );
        cy += 36.0;

        // Mouse keys
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Mouse Keys".into(),
            font_size: 15.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 24.0;
        self.render_toggle_row(
            cmds,
            p,
            x,
            cy,
            width,
            "Enable",
            self.settings.input.mouse_keys.enabled,
        );
        cy += 28.0;
        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Speed",
            &self.settings.input.mouse_keys.speed.to_string(),
        );
        cy += 36.0;

        // Other
        self.render_toggle_row(
            cmds,
            p,
            x,
            cy,
            width,
            "On-Screen Keyboard",
            self.settings.input.on_screen_keyboard,
        );
        cy += 28.0;
        self.render_toggle_row(
            cmds,
            p,
            x,
            cy,
            width,
            "Auto Click",
            self.settings.input.auto_click,
        );
        let _ = cy;
    }

    fn render_audio(&self, cmds: &mut Vec<RenderCommand>, p: &Palette, x: f32, y: f32, width: f32) {
        let mut cy = y;
        let a = &self.settings.audio;

        self.render_toggle_row(cmds, p, x, cy, width, "Visual Alerts", a.visual_alerts);
        cy += 32.0;
        self.render_toggle_row(cmds, p, x, cy, width, "Flash Screen", a.flash_screen);
        cy += 32.0;
        self.render_toggle_row(cmds, p, x, cy, width, "Mono Audio", a.mono_audio);
        cy += 36.0;

        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Captions".into(),
            font_size: 15.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 24.0;
        self.render_toggle_row(cmds, p, x, cy, width, "Show Captions", a.show_captions);
        cy += 28.0;
        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Font Size",
            &format!("{:.0}pt", a.caption_font_size),
        );
        cy += 28.0;
        self.render_toggle_row(cmds, p, x, cy, width, "Background", a.caption_background);
        let _ = cy;
    }

    fn render_reader(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut cy = y;
        let r = &self.settings.screen_reader;

        self.render_toggle_row(cmds, p, x, cy, width, "Screen Reader", r.enabled);
        cy += 36.0;

        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Speech Rate",
            &format!("{}%", r.speech_rate),
        );
        cy += 28.0;
        self.render_label_value(cmds, p, x, cy, width, "Pitch", &format!("{}%", r.pitch));
        cy += 28.0;
        self.render_label_value(cmds, p, x, cy, width, "Volume", &format!("{}%", r.volume));
        cy += 28.0;
        self.render_label_value(cmds, p, x, cy, width, "Verbosity", r.verbosity.label());
        cy += 36.0;

        self.render_toggle_row(
            cmds,
            p,
            x,
            cy,
            width,
            "Read Typed Characters",
            r.read_typed_chars,
        );
        cy += 28.0;
        self.render_toggle_row(
            cmds,
            p,
            x,
            cy,
            width,
            "Read Typed Words",
            r.read_typed_words,
        );
        cy += 28.0;
        self.render_toggle_row(
            cmds,
            p,
            x,
            cy,
            width,
            "Announce Notifications",
            r.announce_notifications,
        );
        let _ = cy;
    }

    fn render_magnifier(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut cy = y;
        let m = &self.settings.magnifier;

        self.render_toggle_row(cmds, p, x, cy, width, "Magnifier", m.enabled);
        cy += 36.0;

        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Zoom Level",
            &format!("{:.1}x", m.zoom_level),
        );
        cy += 28.0;
        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Lens Size",
            &format!("{}px", m.lens_size),
        );
        cy += 36.0;

        self.render_toggle_row(cmds, p, x, cy, width, "Follow Cursor", m.follow_cursor);
        cy += 28.0;
        self.render_toggle_row(cmds, p, x, cy, width, "Follow Focus", m.follow_focus);
        cy += 28.0;
        self.render_toggle_row(
            cmds,
            p,
            x,
            cy,
            width,
            "Smooth Scrolling",
            m.smooth_scrolling,
        );
        cy += 28.0;
        self.render_toggle_row(cmds, p, x, cy, width, "Invert Colors", m.invert_colors);
        let _ = cy;
    }

    fn render_toggle_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
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
            color: p.text,
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
            color: if enabled { p.green } else { p.surface2 },
            corner_radii: CornerRadii::all(11.0),
        });
        let knob_x = if enabled { sw_x + 20.0 } else { sw_x + 2.0 };
        cmds.push(RenderCommand::FillRect {
            x: knob_x,
            y: y + 4.0,
            width: 18.0,
            height: 18.0,
            color: p.text,
            corner_radii: CornerRadii::all(9.0),
        });
    }

    fn render_label_value(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
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
            max_width: Some(width * 0.45),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

impl Default for AccessibilitySettingsUI {
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
    // These tests assert a float equals the exact literal the code under test was
    // handed. That is the assertion meant: a tolerance would let a value that has
    // drifted pass as one that has not.
    #![allow(clippy::float_cmp)]

    use super::*;
    use crate::palette_check::assert_drawn_from;
    use appearance::{AccentColor, readable_on};
    use guitk::color::Color;

    /// A palette whose accent is a member of neither mode's roles.
    ///
    /// The shipped accent *is* `blue`, so a fixture that leaves it alone
    /// cannot tell "this site followed the accent" from "this site drew the
    /// blue role" — the two are the same value. Magenta is in neither
    /// palette, and unlike module 38's calibration page there are no
    /// instruments here for it to collide with.
    ///
    /// It is also chosen for its *brightness*: `readable_on(#FF00FF)` is
    /// 105 luma, below the 140 threshold, so [`Palette::on_accent`] answers
    /// near-**white** in both modes. That is what lets
    /// [`none_of_the_nine_deleted_constants_is_still_drawn`] name `CRUST`
    /// among the forbidden values — with a pale accent the same near-black
    /// would be the *correct* answer and the check would have to drop it.
    fn accented(light: bool) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = Color::from_hex(0x00FF_00FF);
        assert!(
            !p.roles()
                .iter()
                .any(|(n, r)| *n != "accent" && *r == p.accent),
            "the fixture's accent collided with a role, so accent tests would \
             pass for the wrong reason"
        );
        assert_eq!(
            p.on_accent(),
            Color::from_hex(0x00EF_F1F5),
            "the fixture accent stopped being a dark one, so the \
             deleted-constant table can no longer forbid CRUST"
        );
        p
    }

    /// The accents the appearance page actually offers, minus `Custom`.
    ///
    /// [`Palette::on_accent`] is a *step* function of the accent's
    /// brightness, so one fixture accent samples one side of the step and
    /// says nothing about the other. Walking the real menu is what makes
    /// "the lettering is read off the fill" a claim about the function
    /// rather than about one sample of it.
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

    fn wearing(light: bool, accent: AccentColor) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = if light {
            accent.color_light()
        } else {
            accent.color()
        };
        p
    }

    /// A panel with every tab reachable and at least one feature switched on.
    ///
    /// The "*n* features active" line is drawn only when the count is
    /// non-zero, so a fixture at defaults would leave one of the nine
    /// converted sites unrendered and therefore unchecked.
    fn busy() -> AccessibilitySettingsUI {
        let mut ui = AccessibilitySettingsUI::new();
        ui.settings.visual.reduce_motion = true;
        ui.settings.screen_reader.enabled = true;
        ui
    }

    /// Every colour `cmds` puts on the screen, in draw order.
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

    /// The `(fill, lettering)` of every tab in the bar, in draw order.
    ///
    /// Found structurally — a 30-pixel-high fill and the `Text` right after
    /// it — rather than by index. An index into a render is a claim about
    /// layout, and this bar's tab widths depend on the label text, so the
    /// day a tab is renamed an index-based locator starts testing the wrong
    /// command while still passing.
    fn tabs(cmds: &[RenderCommand]) -> Vec<(Color, Color)> {
        let mut out = Vec::new();
        for pair in cmds.windows(2) {
            if let (
                RenderCommand::FillRect {
                    height,
                    color: fill,
                    ..
                },
                RenderCommand::Text { color: ink, .. },
            ) = (&pair[0], &pair[1])
                && *height == 30.0
            {
                out.push((*fill, *ink));
            }
        }
        assert_eq!(
            out.len(),
            A11yTab::ALL.len(),
            "not every tab was found in the render"
        );
        out
    }

    /// Every toggle switch's `(pill, knob)`, in draw order.
    ///
    /// A switch is the 40x22 pill immediately followed by the 18x18 knob.
    fn switches(cmds: &[RenderCommand]) -> Vec<(Color, Color)> {
        let mut out = Vec::new();
        for pair in cmds.windows(2) {
            if let (
                RenderCommand::FillRect {
                    width: pw,
                    height: ph,
                    color: pill,
                    ..
                },
                RenderCommand::FillRect {
                    width: kw,
                    color: knob,
                    ..
                },
            ) = (&pair[0], &pair[1])
                && *pw == 40.0
                && *ph == 22.0
                && *kw == 18.0
            {
                out.push((*pill, *knob));
            }
        }
        out
    }

    /// The colours of the 15pt Bold runs, which is what a section heading is.
    fn headings(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    font_size,
                    font_weight: FontWeightHint::Bold,
                    color,
                    ..
                } if *font_size == 15.0 => Some(*color),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn test_contrast_mode_labels() {
        assert_eq!(ContrastMode::Off.label(), "Off");
        assert_eq!(ContrastMode::HighContrast.label(), "High Contrast");
    }

    #[test]
    fn test_color_filter_labels() {
        assert_eq!(ColorFilter::Off.label(), "Off");
        assert_eq!(
            ColorFilter::Deuteranopia.label(),
            "Red-Green (Deuteranopia)"
        );
    }

    #[test]
    fn test_text_scale_factor() {
        assert_eq!(TextScale::Normal.factor(), 1.0);
        assert_eq!(TextScale::Largest.factor(), 2.0);
    }

    #[test]
    fn test_cursor_indicator_labels() {
        assert_eq!(CursorIndicator::Off.label(), "Off");
        assert_eq!(CursorIndicator::Ring.label(), "Ring");
    }

    #[test]
    fn test_visual_defaults() {
        let v = VisualSettings::default();
        assert_eq!(v.contrast_mode, ContrastMode::Off);
        assert_eq!(v.color_filter, ColorFilter::Off);
        assert!(!v.reduce_motion);
    }

    #[test]
    fn test_sticky_keys_defaults() {
        let s = StickyKeysConfig::default();
        assert!(!s.enabled);
        assert!(s.lock_on_double_press);
    }

    #[test]
    fn test_filter_keys_defaults() {
        let f = FilterKeysConfig::default();
        assert!(!f.enabled);
        assert_eq!(f.acceptance_delay_ms, 300);
    }

    #[test]
    fn test_mouse_keys_defaults() {
        let m = MouseKeysConfig::default();
        assert!(!m.enabled);
        assert_eq!(m.speed, 5);
    }

    #[test]
    fn test_audio_defaults() {
        let a = AudioA11ySettings::default();
        assert!(!a.visual_alerts);
        assert!(!a.mono_audio);
    }

    #[test]
    fn test_reader_verbosity_labels() {
        assert_eq!(ScreenReaderVerbosity::Low.label(), "Low");
        assert_eq!(ScreenReaderVerbosity::High.label(), "High");
    }

    #[test]
    fn test_reader_defaults() {
        let r = ScreenReaderConfig::default();
        assert!(!r.enabled);
        assert_eq!(r.speech_rate, 50);
        assert_eq!(r.verbosity, ScreenReaderVerbosity::Medium);
    }

    #[test]
    fn test_magnifier_defaults() {
        let m = MagnifierConfig::default();
        assert!(!m.enabled);
        assert_eq!(m.zoom_level, 2.0);
    }

    #[test]
    fn test_active_feature_count_none() {
        let s = AccessibilitySettings::default();
        assert_eq!(s.active_feature_count(), 0);
    }

    #[test]
    fn test_active_feature_count_some() {
        let mut s = AccessibilitySettings::default();
        s.visual.reduce_motion = true;
        s.input.sticky_keys.enabled = true;
        s.screen_reader.enabled = true;
        assert_eq!(s.active_feature_count(), 3);
    }

    #[test]
    fn test_active_feature_count_all() {
        let mut s = AccessibilitySettings::default();
        s.visual.contrast_mode = ContrastMode::HighContrast;
        s.visual.color_filter = ColorFilter::Grayscale;
        s.visual.reduce_motion = true;
        s.visual.reduce_transparency = true;
        s.visual.cursor_indicator = CursorIndicator::Ring;
        s.input.sticky_keys.enabled = true;
        s.input.filter_keys.enabled = true;
        s.input.mouse_keys.enabled = true;
        s.input.on_screen_keyboard = true;
        s.input.auto_click = true;
        s.audio.visual_alerts = true;
        s.audio.flash_screen = true;
        s.audio.mono_audio = true;
        s.audio.show_captions = true;
        s.screen_reader.enabled = true;
        s.magnifier.enabled = true;
        assert_eq!(s.active_feature_count(), A11yFeature::ALL.len());
    }

    /// Switch on exactly the named feature.
    ///
    /// The `match` is exhaustive, so this helper cannot fall behind
    /// [`A11yFeature`] the way the old fifteen-arm counter fell behind the
    /// settings pane. That is the whole point of routing the tests below
    /// through it rather than through a hand-written list of fields.
    fn switch_on(s: &mut AccessibilitySettings, feature: A11yFeature) {
        match feature {
            A11yFeature::HighContrast => s.visual.contrast_mode = ContrastMode::HighContrast,
            A11yFeature::ColorFilter => s.visual.color_filter = ColorFilter::Grayscale,
            A11yFeature::ReduceMotion => s.visual.reduce_motion = true,
            A11yFeature::ReduceTransparency => s.visual.reduce_transparency = true,
            A11yFeature::CursorIndicator => s.visual.cursor_indicator = CursorIndicator::Ring,
            A11yFeature::StickyKeys => s.input.sticky_keys.enabled = true,
            A11yFeature::FilterKeys => s.input.filter_keys.enabled = true,
            A11yFeature::MouseKeys => s.input.mouse_keys.enabled = true,
            A11yFeature::OnScreenKeyboard => s.input.on_screen_keyboard = true,
            A11yFeature::AutoClick => s.input.auto_click = true,
            A11yFeature::VisualAlerts => s.audio.visual_alerts = true,
            A11yFeature::FlashScreen => s.audio.flash_screen = true,
            A11yFeature::MonoAudio => s.audio.mono_audio = true,
            A11yFeature::Captions => s.audio.show_captions = true,
            A11yFeature::ScreenReader => s.screen_reader.enabled = true,
            A11yFeature::Magnifier => s.magnifier.enabled = true,
        }
    }

    /// Where each feature sits in [`A11yFeature::ALL`].
    ///
    /// Exhaustive, so a new variant must be given a position; the assertion
    /// in `every_feature_appears_in_all_exactly_once` then checks `ALL`
    /// really holds it there. Between them, a variant cannot be added to the
    /// enum and forgotten in the list.
    fn position_in_all(feature: A11yFeature) -> usize {
        match feature {
            A11yFeature::HighContrast => 0,
            A11yFeature::ColorFilter => 1,
            A11yFeature::ReduceMotion => 2,
            A11yFeature::ReduceTransparency => 3,
            A11yFeature::CursorIndicator => 4,
            A11yFeature::StickyKeys => 5,
            A11yFeature::FilterKeys => 6,
            A11yFeature::MouseKeys => 7,
            A11yFeature::OnScreenKeyboard => 8,
            A11yFeature::AutoClick => 9,
            A11yFeature::VisualAlerts => 10,
            A11yFeature::FlashScreen => 11,
            A11yFeature::MonoAudio => 12,
            A11yFeature::Captions => 13,
            A11yFeature::ScreenReader => 14,
            A11yFeature::Magnifier => 15,
        }
    }

    #[test]
    fn every_feature_appears_in_all_exactly_once() {
        for (i, &feature) in A11yFeature::ALL.iter().enumerate() {
            assert_eq!(position_in_all(feature), i, "{feature:?} is out of place");
        }
        assert_eq!(
            A11yFeature::ALL.len(),
            16,
            "a variant was added to the enum but not to ALL"
        );
    }

    #[test]
    fn switching_on_one_feature_makes_exactly_that_one_active() {
        // The bug this closes: `flash_screen` had a toggle row in the pane
        // and was a peer of two counted features, but the counter's
        // hand-written list omitted it -- so switching it on alone reported
        // "0 features active" and the banner did not appear at all.
        for feature in A11yFeature::ALL {
            let mut s = AccessibilitySettings::default();
            assert!(!s.is_active(feature), "{feature:?} is on by default");
            switch_on(&mut s, feature);
            assert!(s.is_active(feature), "{feature:?} did not switch on");
            assert_eq!(
                s.active_features().collect::<Vec<_>>(),
                vec![feature],
                "switching on {feature:?} moved something else"
            );
            assert_eq!(s.active_feature_count(), 1, "{feature:?}");
        }
    }

    #[test]
    fn switching_features_on_one_at_a_time_counts_them_all() {
        let mut s = AccessibilitySettings::default();
        for (i, feature) in A11yFeature::ALL.into_iter().enumerate() {
            switch_on(&mut s, feature);
            assert_eq!(s.active_feature_count(), i + 1, "after {feature:?}");
        }
        assert_eq!(
            s.active_features().collect::<Vec<_>>(),
            A11yFeature::ALL.to_vec(),
            "active features come back in presentation order"
        );
    }

    #[test]
    fn no_two_features_share_a_label() {
        let mut labels: Vec<&str> = A11yFeature::ALL.iter().map(|f| f.label()).collect();
        labels.sort_unstable();
        let count = labels.len();
        labels.dedup();
        assert_eq!(labels.len(), count, "two features are labelled the same");
    }

    #[test]
    fn test_ui_new() {
        let ui = AccessibilitySettingsUI::new();
        assert_eq!(ui.active_tab, A11yTab::Visual);
    }

    #[test]
    fn test_ui_set_tab() {
        let mut ui = AccessibilitySettingsUI::new();
        ui.set_tab(A11yTab::Magnifier);
        assert_eq!(ui.active_tab, A11yTab::Magnifier);
    }

    #[test]
    fn test_ui_render_visual() {
        let ui = AccessibilitySettingsUI::new();
        let cmds = ui.render(&accented(false), 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_input() {
        let mut ui = AccessibilitySettingsUI::new();
        ui.set_tab(A11yTab::Input);
        let cmds = ui.render(&accented(false), 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_audio() {
        let mut ui = AccessibilitySettingsUI::new();
        ui.set_tab(A11yTab::Audio);
        let cmds = ui.render(&accented(false), 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_reader() {
        let mut ui = AccessibilitySettingsUI::new();
        ui.set_tab(A11yTab::Reader);
        let cmds = ui.render(&accented(false), 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_magnifier() {
        let mut ui = AccessibilitySettingsUI::new();
        ui.set_tab(A11yTab::Magnifier);
        let cmds = ui.render(&accented(false), 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_with_active_features() {
        let mut ui = AccessibilitySettingsUI::new();
        ui.settings.visual.reduce_motion = true;
        ui.settings.screen_reader.enabled = true;
        let cmds = ui.render(&accented(false), 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_tab_labels() {
        assert_eq!(A11yTab::Visual.label(), "Visual");
        assert_eq!(A11yTab::Input.label(), "Input");
        assert_eq!(A11yTab::Audio.label(), "Audio");
        assert_eq!(A11yTab::Reader.label(), "Reader");
        assert_eq!(A11yTab::Magnifier.label(), "Magnifier");
    }

    // ------------------------------------------------------------------
    // The conversion off this module's own copy of the palette
    // ------------------------------------------------------------------

    /// Every value the nine deleted constants held, with the name it had.
    ///
    /// Written out rather than read from `Palette::for_mode(false)` on
    /// purpose: the point is to compare against something the code under
    /// test did not produce.
    const DELETED: [(u32, &str); 9] = [
        (0x001E_1E2E, "BASE"),
        (0x0011_111B, "CRUST"),
        (0x0031_3244, "SURFACE0"),
        (0x0058_5B70, "SURFACE2"),
        (0x00CD_D6F4, "TEXT"),
        (0x00A6_ADC8, "SUBTEXT0"),
        (0x0089_B4FA, "BLUE"),
        (0x00A6_E3A1, "GREEN"),
        (0x00B4_BEFE, "LAVENDER"),
    ];

    /// The broad sweep: no command carries a colour the palette cannot
    /// account for, on any tab, in either mode.
    ///
    /// `derived` is empty because this module computes nothing — every
    /// colour it draws is a role or [`Palette::on_accent`], and `on_accent`
    /// answers a `readable_on` endpoint, which the sweep already allows.
    #[test]
    fn every_colour_the_panel_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = accented(light);
            for &tab in &A11yTab::ALL {
                for mut ui in [AccessibilitySettingsUI::new(), busy()] {
                    ui.set_tab(tab);
                    let cmds = ui.render(&p, 600.0, 800.0);
                    assert_drawn_from(&p, &cmds, &[], "accessibility_settings");
                }
            }
        }
    }

    /// The narrow sweep the broad one cannot do: `CRUST`.
    ///
    /// `assert_drawn_from` has to allow `#11111B` — it is one of the two
    /// answers `readable_on` gives, so a light-mode button with a pale fill
    /// legitimately draws it. That makes the membership sweep structurally
    /// blind to a leftover literal `CRUST`, which is exactly the constant
    /// this module used for its selected-tab lettering. Naming the nine
    /// values explicitly closes the hole, and the fixture's accent is dark
    /// (see [`accented`]) so near-black is never the right answer here.
    #[test]
    fn none_of_the_nine_deleted_constants_is_still_drawn() {
        let p = accented(true);
        for &tab in &A11yTab::ALL {
            let mut ui = busy();
            ui.set_tab(tab);
            for c in colors(&ui.render(&p, 600.0, 800.0)) {
                let rgb = (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
                for (hex, name) in DELETED {
                    assert_ne!(
                        rgb, hex,
                        "the light render of the {tab:?} tab still draws Mocha \
                         {name} — a constant survived the conversion"
                    );
                }
            }
        }
    }

    /// Every site draws the role it claims, in the order it claims it.
    ///
    /// A set-membership table cannot see a *permutation*: swap the label's
    /// colour with the value's and the set of colours on the panel is
    /// unchanged. Pinning the ordered vector is what makes a traded pair
    /// fail, and it costs a literal that has to be re-derived whenever the
    /// layout genuinely changes — which is the point, not the price.
    #[test]
    fn every_site_draws_the_role_it_claims() {
        for light in [false, true] {
            let p = accented(light);

            let mut ui = busy();
            ui.set_tab(A11yTab::Visual);
            let mut want = vec![p.base, p.text, p.green];
            // The tab bar: Visual is chosen, so it is first.
            want.extend([p.accent, p.on_accent()]);
            for _ in 1..A11yTab::ALL.len() {
                want.extend([p.surface0, p.subtext0]);
            }
            // Contrast, Color Filter, Text Size, Cursor Indicator, Cursor Size.
            for _ in 0..5 {
                want.extend([p.subtext0, p.text]);
            }
            // Reduce Motion is on in `busy()`; the other two are off.
            want.extend([p.text, p.green, p.text]);
            want.extend([p.text, p.surface2, p.text]);
            want.extend([p.text, p.surface2, p.text]);
            // Focus Indicator, Text Cursor.
            for _ in 0..2 {
                want.extend([p.subtext0, p.text]);
            }
            assert_eq!(
                colors(&ui.render(&p, 600.0, 800.0)),
                want,
                "the Visual tab's colours are not the roles it claims, in order"
            );

            let mut ui = busy();
            ui.set_tab(A11yTab::Audio);
            let mut want = vec![p.base, p.text, p.green];
            // Audio is the third tab, so two unchosen pairs precede it.
            for _ in 0..2 {
                want.extend([p.surface0, p.subtext0]);
            }
            want.extend([p.accent, p.on_accent()]);
            for _ in 3..A11yTab::ALL.len() {
                want.extend([p.surface0, p.subtext0]);
            }
            // Visual Alerts, Flash Screen, Mono Audio — all off by default.
            for _ in 0..3 {
                want.extend([p.text, p.surface2, p.text]);
            }
            // The "Captions" heading sits between the switches, which is the
            // position a heading-shaped permutation would move.
            want.push(p.lavender);
            want.extend([p.text, p.surface2, p.text]);
            want.extend([p.subtext0, p.text]);
            // `caption_background` is the one audio default that is on.
            want.extend([p.text, p.green, p.text]);
            assert_eq!(
                colors(&ui.render(&p, 600.0, 800.0)),
                want,
                "the Audio tab's colours are not the roles it claims, in order"
            );
        }
    }

    /// The selected tab's lettering is read off its own fill.
    ///
    /// This is the module's real bug fix. The deleted `CRUST` was 8.91:1 on
    /// Mocha `blue` and 3.58:1 on Latte's — below the 4.5:1 floor, on the
    /// accessibility page of all places. Walking the whole accent menu, in
    /// both modes, is what makes this a claim about the *function*:
    /// `readable_on` is a step, and one accent samples one side of it.
    #[test]
    fn the_selected_tab_is_lettered_for_its_own_fill() {
        let mut saw_dark_ink = false;
        let mut saw_light_ink = false;
        for light in [false, true] {
            for accent in OFFERED {
                let p = wearing(light, accent);
                for (i, &tab) in A11yTab::ALL.iter().enumerate() {
                    let mut ui = busy();
                    ui.set_tab(tab);
                    let (fill, ink) = tabs(&ui.render(&p, 600.0, 800.0))[i];
                    assert_eq!(fill, p.accent, "the chosen tab is not the accent");
                    assert_eq!(
                        ink,
                        readable_on(fill),
                        "with the {accent:?} accent the chosen tab's lettering \
                         is not the one legible on its own fill"
                    );
                    if ink == Color::from_hex(0x0011_111B) {
                        saw_dark_ink = true;
                    } else {
                        saw_light_ink = true;
                    }
                }
            }
        }
        assert!(
            saw_dark_ink && saw_light_ink,
            "the offered accents produced only one of readable_on's two \
             answers, so this test would still pass if the lettering were a \
             constant — the fixture set, not the code, needs widening"
        );
    }

    /// Exactly one tab is accented, and it is the chosen one.
    #[test]
    fn exactly_one_tab_is_accented_and_it_is_the_chosen_one() {
        for light in [false, true] {
            let p = accented(light);
            for (i, &tab) in A11yTab::ALL.iter().enumerate() {
                let mut ui = busy();
                ui.set_tab(tab);
                let bar = tabs(&ui.render(&p, 600.0, 800.0));
                let lit: Vec<usize> = bar
                    .iter()
                    .enumerate()
                    .filter(|(_, (fill, _))| *fill == p.accent)
                    .map(|(j, _)| j)
                    .collect();
                assert_eq!(
                    lit,
                    vec![i],
                    "with {tab:?} chosen, the accented tabs were {lit:?}"
                );
                for (j, (fill, ink)) in bar.iter().enumerate() {
                    if j != i {
                        assert_eq!(*fill, p.surface0, "unchosen tab {j} is not surface0");
                        assert_eq!(*ink, p.subtext0, "unchosen tab {j} is not subtext0");
                    }
                }
            }
        }
    }

    /// Green reports *state*; the accent reports *selection*. They are not
    /// the same thing and must not move together.
    ///
    /// A switch that followed the accent would read "on" to every user who
    /// picked a green accent, and would stop reading "on" to everyone else.
    #[test]
    fn green_means_on_and_the_accent_means_chosen() {
        for light in [false, true] {
            let magenta = accented(light);
            let mauve = wearing(light, AccentColor::Mauve);
            let mut ui = busy();
            ui.set_tab(A11yTab::Visual);
            let with_magenta = ui.render(&magenta, 600.0, 800.0);
            let with_mauve = ui.render(&mauve, 600.0, 800.0);

            let pills =
                |c: &[RenderCommand]| -> Vec<Color> { switches(c).iter().map(|s| s.0).collect() };
            assert_eq!(
                pills(&with_magenta),
                pills(&with_mauve),
                "a switch changed colour because the accent changed, so \
                 \"on\" and \"chosen\" have been conflated"
            );

            let sw = switches(&with_magenta);
            assert_eq!(sw.len(), 3, "the Visual tab has three switches");
            assert_eq!(sw[0].0, magenta.green, "Reduce Motion is on and not green");
            assert_eq!(sw[1].0, magenta.surface2, "an off switch is not surface2");
            assert!(
                sw.iter().all(|(_, knob)| *knob == magenta.text),
                "a switch knob is not p.text"
            );

            // The selected tab, by contrast, must move with the accent.
            assert_ne!(
                tabs(&with_magenta)[0].0,
                tabs(&with_mauve)[0].0,
                "the chosen tab did not follow the accent"
            );
        }
    }

    /// The "*n* features active" line is green, and is absent at zero.
    ///
    /// Located by its text rather than its index: an index into a render is
    /// a claim about layout, and this line's presence is conditional, so
    /// everything after it shifts when it disappears.
    #[test]
    fn the_active_feature_line_is_green_and_only_drawn_when_there_is_one() {
        for light in [false, true] {
            let p = accented(light);
            let quiet = AccessibilitySettingsUI::new();
            assert_eq!(
                quiet.settings.active_feature_count(),
                0,
                "the default panel gained an active feature, so this test no \
                 longer checks the zero case"
            );
            assert!(
                !colors(&quiet.render(&p, 600.0, 800.0)).contains(&p.green),
                "a panel with no features enabled still drew something green"
            );

            let line = busy()
                .render(&p, 600.0, 800.0)
                .into_iter()
                .find_map(|c| match c {
                    RenderCommand::Text { text, color, .. }
                        if text.ends_with("features active") =>
                    {
                        Some(color)
                    }
                    _ => None,
                })
                .expect("the active-feature line was not drawn");
            assert_eq!(line, p.green, "the active-feature count is not green");
        }
    }

    /// Every section heading is `p.lavender` at 15pt Bold, on every tab that
    /// has one — and the tabs without headings do not grow any.
    #[test]
    fn the_section_headings_keep_their_hue_in_both_modes() {
        for light in [false, true] {
            let p = accented(light);
            for (tab, want) in [
                (A11yTab::Visual, 0),
                (A11yTab::Input, 3),
                (A11yTab::Audio, 1),
                (A11yTab::Reader, 0),
                (A11yTab::Magnifier, 0),
            ] {
                let mut ui = busy();
                ui.set_tab(tab);
                let h = headings(&ui.render(&p, 600.0, 800.0));
                assert_eq!(
                    h.len(),
                    want,
                    "the {tab:?} tab drew {} section headings, expected {want}",
                    h.len()
                );
                assert!(
                    h.iter().all(|c| *c == p.lavender),
                    "a section heading on the {tab:?} tab is not lavender: {h:?}"
                );
            }
        }
    }

    /// No site is mode-invariant.
    ///
    /// Stronger than the deleted-constant table, which only catches the nine
    /// *Mocha* values: this catches any site left reading a constant at all,
    /// including one that happens to be a Latte value. It holds because
    /// every role differs between the two palettes, and the fixture accent
    /// differs too.
    #[test]
    fn every_site_changes_when_the_mode_does() {
        for &tab in &A11yTab::ALL {
            let mut ui = busy();
            ui.set_tab(tab);
            let dark = colors(&ui.render(&wearing(false, AccentColor::Mauve), 600.0, 800.0));
            let lite = colors(&ui.render(&wearing(true, AccentColor::Mauve), 600.0, 800.0));
            assert_eq!(
                dark.len(),
                lite.len(),
                "the two modes drew different numbers of coloured commands"
            );
            for (i, (d, l)) in dark.iter().zip(&lite).enumerate() {
                assert_ne!(
                    d, l,
                    "colour {i} of the {tab:?} tab is identical in both modes, \
                     so that site is not reading the palette"
                );
            }
        }
    }
}
