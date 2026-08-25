//! The desktop's input settings: the model, not the panel.
//!
//! How fast the pointer moves, which button is primary, how long a user has
//! between the two clicks of a double click, how quickly a held key repeats.
//! One user preference cannot have two owners: the compositor *applies* these
//! values, the Settings application *edits* them, and the shell's input panel
//! renders them — so the types, their configuration-file spellings, the file's
//! location and the way it is replaced all have to be one definition rather
//! than a copy in each process.
//!
//! This crate is the counterpart of [`appearance`], and exists for the same
//! reason and in the same shape: a third crate that the shell, the compositor
//! and the Settings application can all depend on without any two of them
//! depending on each other.
//!
//! # What was here before
//!
//! These types lived in `desktop::mouse_settings` next to the panel that draws
//! them, with **no persistence at all** — no save, no load, no path, no
//! serialisation, and no consumer. A user could drag the double-click slider,
//! watch the number change, and find that nothing on the screen behaved any
//! differently and that the value was gone at the next login. See
//! `known-issues.md` `TD-C-THE-MOUSE-SETTINGS-PANEL-REACHES-NOTHING`.
//! `desktop::mouse_settings` now re-exports this crate and keeps only the
//! rendering.
//!
//! # What deliberately stays out
//!
//! Rendering, and the *interpretation* of the values. This crate knows that
//! `speed` runs from −10 to +10; it does not know how many pixels that is,
//! because that is the compositor's pointer pipeline and it may reasonably
//! differ between a mouse and a trackpad. It knows a double-click window is a
//! number of milliseconds; the compositor is what compares it against two
//! timestamps.
//!
//! [`appearance`]: ../appearance/index.html

/// Where settings files live and how they are replaced.
///
/// Re-exported for the same reason `appearance` re-exports it: two processes
/// writing one file must agree not only on what the keys mean but on which
/// file it is and how it is replaced, so the location is part of this crate's
/// contract rather than a detail of it. It is also what a caller's tests reach
/// for — `inputsettings::config::testing::with_scratch_config` — so that they
/// do not write into the developer's own `~/.config/slateos`.
pub use settingsfile as config;

use settingsfile::yaml_enum;
use yamldoc::Document;

// ============================================================================
// The double-click window
// ============================================================================
//
// These three are `pub` and named rather than left as literals inside
// `MouseConfig` because three separate processes have to agree on them: this
// crate clamps a value being stored, the compositor clamps the value it was
// handed before timing anything against it, and the Settings application draws
// a slider whose two ends *are* this range. When the compositor kept its own
// copy of the numbers, nothing anywhere failed if one of the three moved.

/// The shortest double-click window that can be set, in milliseconds.
///
/// A floor rather than zero: below about a tenth of a second the gesture is not
/// physically achievable, so a setting under it would not be a fast double
/// click but an impossible one.
pub const MIN_DOUBLE_CLICK_MS: u32 = 100;

/// The longest double-click window that can be set, in milliseconds.
///
/// Above two seconds, two deliberately separate clicks start being read as one
/// double click — the setting stops being slow and starts being wrong.
pub const MAX_DOUBLE_CLICK_MS: u32 = 2000;

/// The double-click window a user who has never set one gets.
pub const DEFAULT_DOUBLE_CLICK_MS: u32 = 400;

// The default has to be a value the setter would accept, or a user who has
// never touched the setting would be given one the Settings slider cannot
// return to and `validate` would silently move. Checked at compile time rather
// than in a test, because there is no run in which it could be true here and
// false somewhere else.
const _: () = assert!(MIN_DOUBLE_CLICK_MS < DEFAULT_DOUBLE_CLICK_MS);
const _: () = assert!(DEFAULT_DOUBLE_CLICK_MS < MAX_DOUBLE_CLICK_MS);

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
    /// The name shown to the user.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Flat => "Flat (no acceleration)",
            Self::Adaptive => "Adaptive",
            Self::Custom => "Custom curve",
        }
    }

    /// Every profile, in the order a chooser should offer them.
    pub const ALL: [Self; 3] = [Self::Flat, Self::Adaptive, Self::Custom];
}

yaml_enum!(AccelProfile {
    Flat => "flat",
    Adaptive => "adaptive",
    Custom => "custom",
});

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
    /// The name shown to the user.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Lines => "Lines",
            Self::Pages => "Pages",
            Self::Smooth => "Smooth",
        }
    }

    /// Every mode, in the order a chooser should offer them.
    pub const ALL: [Self; 3] = [Self::Lines, Self::Pages, Self::Smooth];
}

yaml_enum!(ScrollMode {
    Lines => "lines",
    Pages => "pages",
    Smooth => "smooth",
});

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
    /// The name shown to the user.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::RightHanded => "Right-handed",
            Self::LeftHanded => "Left-handed",
        }
    }

    /// Both mappings, in the order a chooser should offer them.
    pub const ALL: [Self; 2] = [Self::RightHanded, Self::LeftHanded];
}

yaml_enum!(ButtonMapping {
    RightHanded => "right_handed",
    LeftHanded => "left_handed",
});

// ============================================================================
// Keyboard settings
// ============================================================================

/// Keyboard configuration: which layout, and how a held key repeats.
///
/// Named for the keyboard rather than for the repeat rate, which is what it
/// held first. The layout belongs in the same struct and travels the same
/// route: both are things a user changes in Settings and expects to take
/// effect on the next keystroke, and both are read by whoever turns a scancode
/// into a letter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyboardConfig {
    /// Id of the keyboard layout — `"us-qwerty"`, `"dvorak"`, `"de-qwertz"`.
    /// See [`keylayout::builtins`] for the catalogue.
    ///
    /// Stored as written and **not** validated against that catalogue, on
    /// purpose. A layout id this build does not recognise is far more likely
    /// to be one a newer build added — or one the user is about to install —
    /// than a typo, and rewriting it to `us-qwerty` on the next save would
    /// destroy the setting rather than fall back from it. Falling back is the
    /// reader's job, and [`keylayout::by_id`] returning `None` is where it
    /// happens.
    pub layout: String,
    /// Delay before repeat starts, in milliseconds (150–2000).
    pub repeat_delay_ms: u32,
    /// Interval between repeated keystrokes, in milliseconds (10–500).
    pub repeat_interval_ms: u32,
    /// Whether key repeat is enabled at all.
    pub enabled: bool,
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            layout: keylayout::DEFAULT_ID.to_string(),
            repeat_delay_ms: 500,
            repeat_interval_ms: 30,
            enabled: true,
        }
    }
}

impl KeyboardConfig {
    /// Set the delay before a held key starts repeating, clamped to 150–2000 ms.
    pub fn set_delay(&mut self, ms: u32) {
        self.repeat_delay_ms = ms.clamp(150, 2000);
    }

    /// Set the interval between repeats, clamped to 10–500 ms.
    pub fn set_interval(&mut self, ms: u32) {
        self.repeat_interval_ms = ms.clamp(10, 500);
    }

    /// Force every field back into its documented range.
    ///
    /// The file is user-editable and nothing stops someone typing a repeat
    /// delay of zero, which would make a single keypress fill the screen.
    pub fn validate(&mut self) {
        let (delay, interval) = (self.repeat_delay_ms, self.repeat_interval_ms);
        self.set_delay(delay);
        self.set_interval(interval);
    }
}

// ============================================================================
// Mouse settings
// ============================================================================

/// Full mouse configuration.
#[derive(Clone, Debug, PartialEq)]
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
    /// Double-click window in milliseconds
    /// ([`MIN_DOUBLE_CLICK_MS`]–[`MAX_DOUBLE_CLICK_MS`]).
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
            double_click_ms: DEFAULT_DOUBLE_CLICK_MS,
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
    /// Set the pointer speed factor, clamped to −10..=10.
    pub fn set_speed(&mut self, speed: i32) {
        self.speed = speed.clamp(-10, 10);
    }

    /// Set the double-click window, clamped to
    /// [`MIN_DOUBLE_CLICK_MS`]..=[`MAX_DOUBLE_CLICK_MS`].
    pub fn set_double_click_ms(&mut self, ms: u32) {
        self.double_click_ms = ms.clamp(MIN_DOUBLE_CLICK_MS, MAX_DOUBLE_CLICK_MS);
    }

    /// Set the lines scrolled per notch, clamped to 1–20.
    pub fn set_scroll_lines(&mut self, lines: u32) {
        self.scroll_lines = lines.clamp(1, 20);
    }

    /// Set the smooth-scroll multiplier, clamped to 0.1–5.0.
    pub fn set_scroll_speed(&mut self, speed: f32) {
        self.scroll_speed = speed.clamp(0.1, 5.0);
    }

    /// Set the cursor size in pixels, clamped to 16–128.
    pub fn set_cursor_size(&mut self, size: u32) {
        self.cursor_size = size.clamp(16, 128);
    }

    /// Set the cursor trail length, clamped to 1–10.
    pub fn set_trail_length(&mut self, len: u32) {
        self.trail_length = len.clamp(1, 10);
    }

    /// Set the custom acceleration gain, clamped to 0.1–10.0.
    pub fn set_accel_gain(&mut self, gain: f32) {
        self.accel_gain = gain.clamp(0.1, 10.0);
    }

    /// Set the custom acceleration threshold, capped at 50.
    pub fn set_accel_threshold(&mut self, thr: u32) {
        self.accel_threshold = thr.min(50);
    }

    /// Force every field back into its documented range.
    ///
    /// Every setter clamps, so this is only reachable by a field assignment or
    /// by a value read out of a hand-edited file — which is exactly what it is
    /// for. A NaN scroll speed survives `clamp` (it returns the input), so it
    /// is turned into the default rather than passed on to something that will
    /// multiply a scroll delta by it.
    pub fn validate(&mut self) {
        let this = &mut *self;
        this.set_speed(this.speed);
        this.set_double_click_ms(this.double_click_ms);
        this.set_scroll_lines(this.scroll_lines);
        this.set_cursor_size(this.cursor_size);
        this.set_trail_length(this.trail_length);
        this.set_accel_threshold(this.accel_threshold);
        if this.scroll_speed.is_nan() {
            this.scroll_speed = 1.0;
        }
        this.set_scroll_speed(this.scroll_speed);
        if this.accel_gain.is_nan() {
            this.accel_gain = 1.0;
        }
        this.set_accel_gain(this.accel_gain);
    }
}

// ============================================================================
// Combined input settings
// ============================================================================

/// Combined mouse + keyboard input settings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputSettings {
    /// Everything about the pointer.
    pub mouse: MouseConfig,
    /// Everything about the keyboard: the layout, and how a held key repeats.
    pub keyboard: KeyboardConfig,
}

impl InputSettings {
    /// The defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Restore the mouse half to its defaults, leaving the keyboard alone.
    pub fn reset_mouse(&mut self) {
        self.mouse = MouseConfig::default();
    }

    /// Restore the keyboard half to its defaults, leaving the mouse alone.
    pub fn reset_keyboard(&mut self) {
        self.keyboard = KeyboardConfig::default();
    }

    /// Restore everything to its defaults.
    pub fn reset_all(&mut self) {
        *self = Self::default();
    }

    /// Force every field of both halves back into range.
    pub fn validate(&mut self) {
        self.mouse.validate();
        self.keyboard.validate();
    }
}

// ============================================================================
// The configuration file
// ============================================================================

/// Read a value if the file has one, otherwise keep what is already there.
///
/// This is the whole reason settings are read into a `Default` rather than
/// built from the file: a key the user has never touched, or one written by a
/// newer version and since removed, leaves the field at its default instead of
/// zeroing it.
macro_rules! read_into {
    ($slot:expr, $value:expr) => {
        if let Some(value) = $value {
            $slot = value;
        }
    };
}

impl InputSettings {
    /// Read settings from a configuration document.
    ///
    /// Every key is optional and every unreadable value is ignored, so a
    /// missing file, a partial file and a file from a different version all
    /// produce a usable result. The outcome is always [`validate`]d, because
    /// the file is user-editable and nothing stops someone typing a
    /// double-click window of zero.
    ///
    /// [`validate`]: Self::validate
    #[must_use]
    pub fn read_from(doc: &Document) -> Self {
        let mut s = Self::default();

        // A pointer speed outside i32 is not a number this UI can mean;
        // `validate` clamps the rest of the range. Same for the millisecond
        // and pixel counts below, which are `u32` in the model because a
        // negative delay is not a delay.
        read_into!(
            s.mouse.speed,
            doc.get_i64(&["pointer", "speed"])
                .and_then(|v| i32::try_from(v).ok())
        );
        read_into!(
            s.mouse.accel_profile,
            doc.get_str(&["pointer", "accel_profile"])
                .and_then(|v| AccelProfile::from_yaml_name(&v))
        );
        read_into!(
            s.mouse.accel_gain,
            doc.get_f64(&["pointer", "accel_gain"]).map(|v| v as f32)
        );
        read_into!(
            s.mouse.accel_threshold,
            doc.get_i64(&["pointer", "accel_threshold"])
                .and_then(|v| u32::try_from(v).ok())
        );

        read_into!(
            s.mouse.button_mapping,
            doc.get_str(&["buttons", "mapping"])
                .and_then(|v| ButtonMapping::from_yaml_name(&v))
        );
        read_into!(
            s.mouse.double_click_ms,
            doc.get_i64(&["buttons", "double_click_ms"])
                .and_then(|v| u32::try_from(v).ok())
        );

        read_into!(
            s.mouse.scroll_mode,
            doc.get_str(&["scrolling", "mode"])
                .and_then(|v| ScrollMode::from_yaml_name(&v))
        );
        read_into!(
            s.mouse.scroll_lines,
            doc.get_i64(&["scrolling", "lines"])
                .and_then(|v| u32::try_from(v).ok())
        );
        read_into!(
            s.mouse.scroll_speed,
            doc.get_f64(&["scrolling", "speed"]).map(|v| v as f32)
        );
        read_into!(
            s.mouse.natural_scroll,
            doc.get_bool(&["scrolling", "natural"])
        );

        read_into!(
            s.mouse.cursor_size,
            doc.get_i64(&["cursor", "size"])
                .and_then(|v| u32::try_from(v).ok())
        );
        read_into!(
            s.mouse.locate_on_ctrl,
            doc.get_bool(&["cursor", "locate_on_ctrl"])
        );
        read_into!(
            s.mouse.hide_while_typing,
            doc.get_bool(&["cursor", "hide_while_typing"])
        );
        read_into!(s.mouse.show_trail, doc.get_bool(&["cursor", "trail"]));
        read_into!(
            s.mouse.trail_length,
            doc.get_i64(&["cursor", "trail_length"])
                .and_then(|v| u32::try_from(v).ok())
        );

        read_into!(
            s.keyboard.layout,
            doc.get_str(&["keyboard", "layout"])
                // An empty value means "the key is present but says nothing",
                // which is not the same as naming a layout called "". Taking
                // it literally would leave the reader with an id that can
                // never resolve, so it is treated as absent and the default
                // stands.
                .filter(|v| !v.trim().is_empty())
                .map(|v| v.trim().to_string())
        );
        read_into!(s.keyboard.enabled, doc.get_bool(&["keyboard", "repeat"]));
        read_into!(
            s.keyboard.repeat_delay_ms,
            doc.get_i64(&["keyboard", "delay_ms"])
                .and_then(|v| u32::try_from(v).ok())
        );
        read_into!(
            s.keyboard.repeat_interval_ms,
            doc.get_i64(&["keyboard", "interval_ms"])
                .and_then(|v| u32::try_from(v).ok())
        );

        s.validate();
        s
    }

    /// Write these settings into a configuration document, leaving every
    /// comment, blank line and unrelated key in it exactly as it was.
    pub fn write_into(&self, doc: &mut Document) {
        doc.set_i64(&["pointer", "speed"], i64::from(self.mouse.speed));
        doc.set_str(
            &["pointer", "accel_profile"],
            self.mouse.accel_profile.yaml_name(),
        );
        doc.set_f64(&["pointer", "accel_gain"], f64::from(self.mouse.accel_gain));
        doc.set_i64(
            &["pointer", "accel_threshold"],
            i64::from(self.mouse.accel_threshold),
        );

        doc.set_str(
            &["buttons", "mapping"],
            self.mouse.button_mapping.yaml_name(),
        );
        doc.set_i64(
            &["buttons", "double_click_ms"],
            i64::from(self.mouse.double_click_ms),
        );

        doc.set_str(&["scrolling", "mode"], self.mouse.scroll_mode.yaml_name());
        doc.set_i64(&["scrolling", "lines"], i64::from(self.mouse.scroll_lines));
        doc.set_f64(&["scrolling", "speed"], f64::from(self.mouse.scroll_speed));
        doc.set_bool(&["scrolling", "natural"], self.mouse.natural_scroll);

        doc.set_i64(&["cursor", "size"], i64::from(self.mouse.cursor_size));
        doc.set_bool(&["cursor", "locate_on_ctrl"], self.mouse.locate_on_ctrl);
        doc.set_bool(
            &["cursor", "hide_while_typing"],
            self.mouse.hide_while_typing,
        );
        doc.set_bool(&["cursor", "trail"], self.mouse.show_trail);
        doc.set_i64(
            &["cursor", "trail_length"],
            i64::from(self.mouse.trail_length),
        );

        doc.set_str(&["keyboard", "layout"], &self.keyboard.layout);
        doc.set_bool(&["keyboard", "repeat"], self.keyboard.enabled);
        doc.set_i64(
            &["keyboard", "delay_ms"],
            i64::from(self.keyboard.repeat_delay_ms),
        );
        doc.set_i64(
            &["keyboard", "interval_ms"],
            i64::from(self.keyboard.repeat_interval_ms),
        );
    }
}

/// The settings group these preferences live in — `input.yaml` in the user's
/// configuration directory.
///
/// The *name* is as much a part of the shared contract as the schema is: two
/// processes that agree on every key but disagree about which file holds them
/// have simply written two files.
pub const CONFIG_NAME: &str = "input";

/// The user's input settings together with the document they came from.
///
/// The pair is a type rather than two fields because keeping them together is
/// an invariant, not a convenience: a save must splice the changed values back
/// into the document that was read, since that document carries everything
/// this model does not — the user's comments, their blank lines, their key
/// order, and any setting belonging to a different version of the desktop.
/// Rebuilding the file from [`InputSettings`] alone silently deletes all of it.
pub struct InputFile {
    /// The settings being edited. Public because the front ends bind controls
    /// straight to the fields.
    pub settings: InputSettings,
    /// The file as read, kept whole. See the type's documentation.
    doc: Document,
}

impl Default for InputFile {
    fn default() -> Self {
        Self::new()
    }
}

impl InputFile {
    /// The defaults, backed by an empty document.
    ///
    /// Deliberately does *not* read the filesystem: a constructor that
    /// consulted `$HOME` would make every caller's tests depend on the machine
    /// running them. [`load`](Self::load) does the I/O.
    #[must_use]
    pub fn new() -> Self {
        Self {
            settings: InputSettings::default(),
            doc: Document::new(),
        }
    }

    /// Read the user's saved settings from `input.yaml`.
    ///
    /// A missing or unreadable file yields the defaults — the ordinary state
    /// on a fresh install, not an error to report to someone who has simply
    /// never changed a setting.
    #[must_use]
    pub fn load() -> Self {
        Self::from_document(settingsfile::load(CONFIG_NAME))
    }

    /// Open on an already-read document. Split out from [`load`](Self::load)
    /// so the format can be exercised without a filesystem.
    #[must_use]
    pub fn from_document(doc: Document) -> Self {
        Self {
            settings: InputSettings::read_from(&doc),
            doc,
        }
    }

    /// Fold the current settings into the document without touching the
    /// filesystem, and return it.
    pub fn apply(&mut self) -> &Document {
        self.settings.write_into(&mut self.doc);
        &self.doc
    }

    /// The document as it stands, without folding in pending changes.
    #[must_use]
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// Write the current settings to `input.yaml`, atomically.
    ///
    /// # Errors
    ///
    /// If there is no configuration directory, or the file cannot be written.
    pub fn save(&mut self) -> std::io::Result<()> {
        self.apply();
        settingsfile::store(CONFIG_NAME, &self.doc)
    }
}

// Panicking on bad data is the point of a test, and a test that asserts a
// default is `1.0` means exactly 1.0 — the float comparison is the assertion,
// not an approximation mistake.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    /// The published double-click numbers, pinned as literals.
    ///
    /// The compositor and the Settings slider both read these constants now
    /// rather than keeping their own copies, so nothing else in the tree can
    /// disagree with them — which is exactly why the *values* need pinning
    /// here. Changing 400 silently changes the behaviour of every user who
    /// never touched the setting, and changing 100 or 2000 silently moves the
    /// ends of a slider that people have already dragged to a position.
    #[test]
    fn the_double_click_window_keeps_its_published_range() {
        assert_eq!(MIN_DOUBLE_CLICK_MS, 100);
        assert_eq!(MAX_DOUBLE_CLICK_MS, 2000);
        assert_eq!(DEFAULT_DOUBLE_CLICK_MS, 400);
        assert_eq!(
            MouseConfig::default().double_click_ms,
            DEFAULT_DOUBLE_CLICK_MS
        );
    }

    #[test]
    fn every_setter_clamps() {
        let mut m = MouseConfig::default();
        m.set_speed(9999);
        assert_eq!(m.speed, 10);
        m.set_speed(-9999);
        assert_eq!(m.speed, -10);
        m.set_double_click_ms(0);
        assert_eq!(m.double_click_ms, 100);
        m.set_double_click_ms(99_999);
        assert_eq!(m.double_click_ms, 2000);
        m.set_scroll_lines(0);
        assert_eq!(m.scroll_lines, 1);
        m.set_scroll_speed(0.0);
        assert_eq!(m.scroll_speed, 0.1);
        m.set_cursor_size(0);
        assert_eq!(m.cursor_size, 16);
        m.set_trail_length(0);
        assert_eq!(m.trail_length, 1);
        m.set_accel_gain(0.0);
        assert_eq!(m.accel_gain, 0.1);
        m.set_accel_threshold(9999);
        assert_eq!(m.accel_threshold, 50);

        let mut k = KeyboardConfig::default();
        k.set_delay(0);
        assert_eq!(k.repeat_delay_ms, 150);
        k.set_interval(0);
        assert_eq!(k.repeat_interval_ms, 10);
    }

    #[test]
    fn a_round_trip_preserves_every_field() {
        let mut settings = InputSettings::default();
        settings.mouse.speed = 7;
        settings.mouse.accel_profile = AccelProfile::Flat;
        settings.mouse.accel_gain = 2.5;
        settings.mouse.accel_threshold = 12;
        settings.mouse.button_mapping = ButtonMapping::LeftHanded;
        settings.mouse.double_click_ms = 750;
        settings.mouse.scroll_mode = ScrollMode::Smooth;
        settings.mouse.scroll_lines = 9;
        settings.mouse.scroll_speed = 2.0;
        settings.mouse.natural_scroll = true;
        settings.mouse.cursor_size = 48;
        settings.mouse.locate_on_ctrl = true;
        settings.mouse.hide_while_typing = true;
        settings.mouse.show_trail = true;
        settings.mouse.trail_length = 6;
        settings.keyboard.layout = "dvorak".to_string();
        settings.keyboard.enabled = false;
        settings.keyboard.repeat_delay_ms = 250;
        settings.keyboard.repeat_interval_ms = 15;

        let mut doc = Document::new();
        settings.write_into(&mut doc);
        assert_eq!(InputSettings::read_from(&doc), settings);
    }

    #[test]
    fn a_layout_this_build_does_not_know_survives_a_round_trip_unchanged() {
        // The setting is a *name*, and this crate is not the catalogue. An id
        // from a newer build, or one whose layout is not installed yet, must
        // come back out of the file exactly as it went in — rewriting it to
        // the default on the next save would quietly discard the user's
        // choice, and they would find out by watching Settings forget it.
        let mut settings = InputSettings::default();
        settings.keyboard.layout = "kl-invented".to_string();

        let mut doc = Document::new();
        settings.write_into(&mut doc);
        let mut read = InputSettings::read_from(&doc);
        read.validate();

        assert_eq!(read.keyboard.layout, "kl-invented");
    }

    #[test]
    fn a_blank_layout_name_is_treated_as_absent_rather_than_as_a_layout() {
        // `layout:` with nothing after it is a user who cleared the field, not
        // a user who named a layout called "". Taken literally it would be an
        // id nothing can ever resolve, and the keyboard would fall back on
        // every keystroke instead of once.
        let mut doc = Document::new();
        doc.set_str(&["keyboard", "layout"], "   ");
        assert_eq!(
            InputSettings::read_from(&doc).keyboard.layout,
            keylayout::DEFAULT_ID
        );
    }

    #[test]
    fn the_default_layout_is_one_that_exists() {
        // The point of taking the dependency on `keylayout` rather than
        // spelling the id here: a default naming a layout the catalogue does
        // not contain would leave a fresh install falling back on every key.
        assert!(keylayout::by_id(&KeyboardConfig::default().layout).is_some());
    }

    #[test]
    fn an_empty_document_reads_as_the_defaults() {
        assert_eq!(
            InputSettings::read_from(&Document::new()),
            InputSettings::default()
        );
    }

    #[test]
    fn an_unknown_spelling_falls_back_to_the_default() {
        // A file written by a newer desktop must not stop this one starting.
        let doc = Document::parse("pointer:\n  accel_profile: quantum\n");
        assert_eq!(
            InputSettings::read_from(&doc).mouse.accel_profile,
            AccelProfile::Adaptive
        );
    }

    #[test]
    fn a_hand_edited_file_is_clamped_not_obeyed() {
        let doc = Document::parse(
            "buttons:\n  double_click_ms: 0\npointer:\n  speed: 500\nkeyboard:\n  delay_ms: 1\n",
        );
        let s = InputSettings::read_from(&doc);
        assert_eq!(s.mouse.double_click_ms, 100);
        assert_eq!(s.mouse.speed, 10);
        assert_eq!(s.keyboard.repeat_delay_ms, 150);
    }

    #[test]
    fn a_negative_millisecond_count_is_ignored_rather_than_wrapped() {
        // `u32::try_from(-1)` fails, which must leave the default in place —
        // not wrap round to four billion milliseconds.
        let doc = Document::parse("buttons:\n  double_click_ms: -1\n");
        assert_eq!(InputSettings::read_from(&doc).mouse.double_click_ms, 400);
    }

    #[test]
    fn a_nan_scroll_speed_becomes_the_default() {
        // `f32::clamp` returns NaN unchanged, so the clamp alone is not enough;
        // a NaN reaching the compositor would make every scroll delta NaN.
        let mut m = MouseConfig {
            scroll_speed: f32::NAN,
            accel_gain: f32::NAN,
            ..MouseConfig::default()
        };
        m.validate();
        assert_eq!(m.scroll_speed, 1.0);
        assert_eq!(m.accel_gain, 1.0);
    }

    #[test]
    fn saving_keeps_the_users_comments_and_unknown_keys() {
        let text = "# my mouse\nbuttons:\n  double_click_ms: 400  # ms\nsomething_else: 1\n";
        let mut file = InputFile::from_document(Document::parse(text));
        file.settings.mouse.double_click_ms = 900;
        let out = file.apply().to_text();
        assert!(out.contains("# my mouse"), "{out}");
        assert!(out.contains("# ms"), "{out}");
        assert!(out.contains("something_else: 1"), "{out}");
        assert!(out.contains("double_click_ms: 900"), "{out}");
    }

    #[test]
    fn a_new_file_is_the_defaults() {
        let file = InputFile::new();
        assert_eq!(file.settings, InputSettings::default());
        assert!(file.document().is_empty());
    }

    #[test]
    fn resets_are_independent() {
        let mut s = InputSettings::new();
        s.mouse.double_click_ms = 900;
        s.keyboard.repeat_delay_ms = 250;
        s.reset_mouse();
        assert_eq!(s.mouse.double_click_ms, 400);
        assert_eq!(s.keyboard.repeat_delay_ms, 250);
        s.reset_keyboard();
        assert_eq!(s.keyboard.repeat_delay_ms, 500);
    }

    #[test]
    fn config_spellings_are_not_labels() {
        // The label is UI text and is free to be reworded; the spelling is the
        // file format. If they were the same function, improving the wording
        // would silently reset every user's saved choice.
        assert_eq!(AccelProfile::Flat.yaml_name(), "flat");
        assert_eq!(AccelProfile::Flat.label(), "Flat (no acceleration)");
        assert_eq!(
            ButtonMapping::from_yaml_name("left_handed"),
            Some(ButtonMapping::LeftHanded)
        );
        for p in AccelProfile::ALL {
            assert_eq!(AccelProfile::from_yaml_name(p.yaml_name()), Some(p));
        }
        for m in ScrollMode::ALL {
            assert_eq!(ScrollMode::from_yaml_name(m.yaml_name()), Some(m));
        }
        for b in ButtonMapping::ALL {
            assert_eq!(ButtonMapping::from_yaml_name(b.yaml_name()), Some(b));
        }
    }

    #[test]
    fn a_saved_file_reloads_as_itself() {
        config::testing::with_scratch_config("inputsettings-roundtrip", |root| {
            let mut file = InputFile::load();
            assert_eq!(file.settings, InputSettings::default());
            file.settings.mouse.double_click_ms = 650;
            file.settings.mouse.button_mapping = ButtonMapping::LeftHanded;
            file.save().expect("save");

            let path = config::testing::scratch_path(root, CONFIG_NAME);
            assert!(path.exists(), "input.yaml should be at {}", path.display());

            let reloaded = InputFile::load();
            assert_eq!(reloaded.settings.mouse.double_click_ms, 650);
            assert_eq!(
                reloaded.settings.mouse.button_mapping,
                ButtonMapping::LeftHanded
            );
        });
    }
}
