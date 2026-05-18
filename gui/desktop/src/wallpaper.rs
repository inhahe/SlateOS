//! Wallpaper Manager for the OurOS desktop shell.
//!
//! Provides desktop background rendering with multiple modes: solid color,
//! single image, slideshow, and dynamic time-of-day gradients. Supports
//! configurable image fitting, shuffle, history navigation, and config
//! persistence via a simple key=value text format.
//!
//! # Integration
//!
//! ```ignore
//! let mut wp = WallpaperManager::new();
//! wp.set_slideshow("/wallpapers", 300, true);
//!
//! // Each frame:
//! let changed = wp.tick(current_time_secs);
//! let commands = wp.get_render_commands(screen_w, screen_h);
//!
//! // Manual controls:
//! wp.next_wallpaper();
//! wp.previous_wallpaper();
//! wp.random_wallpaper();
//! ```

use guitk::color::Color;
use guitk::render::RenderCommand;
use guitk::style::CornerRadii;

use std::fmt;

// ============================================================================
// Theme -- Catppuccin Mocha palette constants
// ============================================================================

mod palette {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const LAVENDER: Color = Color::from_hex(0xB4BEFE);
}

// ============================================================================
// Configuration error type
// ============================================================================

/// Errors that can occur when loading or saving wallpaper configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// A required key is missing.
    MissingKey(String),
    /// A value could not be parsed.
    InvalidValue(String),
    /// The mode string is not recognized.
    UnknownMode(String),
    /// The fit string is not recognized.
    UnknownFit(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingKey(k) => write!(f, "missing key: {k}"),
            Self::InvalidValue(v) => write!(f, "invalid value: {v}"),
            Self::UnknownMode(m) => write!(f, "unknown wallpaper mode: {m}"),
            Self::UnknownFit(v) => write!(f, "unknown image fit: {v}"),
        }
    }
}

// ============================================================================
// WallpaperMode
// ============================================================================

/// How the desktop background is produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WallpaperMode {
    /// A single flat color fills the entire background.
    SolidColor,
    /// A single image is displayed.
    SingleImage,
    /// Images from a directory rotate on a timer.
    Slideshow,
    /// A smooth color gradient that shifts based on time-of-day.
    Dynamic,
}

impl WallpaperMode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::SolidColor => "solid",
            Self::SingleImage => "image",
            Self::Slideshow => "slideshow",
            Self::Dynamic => "dynamic",
        }
    }

    fn from_str_config(s: &str) -> Result<Self, ConfigError> {
        match s.trim() {
            "solid" => Ok(Self::SolidColor),
            "image" => Ok(Self::SingleImage),
            "slideshow" => Ok(Self::Slideshow),
            "dynamic" => Ok(Self::Dynamic),
            other => Err(ConfigError::UnknownMode(other.to_string())),
        }
    }
}

// ============================================================================
// ImageFit
// ============================================================================

/// How an image is scaled/positioned within the display area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFit {
    /// Scale to cover the entire area, cropping if necessary.
    Fill,
    /// Scale to fit within the area, letterboxing if necessary.
    Fit,
    /// Stretch to exactly match the area (may distort aspect ratio).
    Stretch,
    /// Repeat the image in a tile pattern.
    Tile,
    /// Center the image at native size (no scaling).
    Center,
    /// Span the image across all monitors (multi-monitor setups).
    Span,
}

impl ImageFit {
    fn as_str(self) -> &'static str {
        match self {
            Self::Fill => "fill",
            Self::Fit => "fit",
            Self::Stretch => "stretch",
            Self::Tile => "tile",
            Self::Center => "center",
            Self::Span => "span",
        }
    }

    fn from_str_config(s: &str) -> Result<Self, ConfigError> {
        match s.trim() {
            "fill" => Ok(Self::Fill),
            "fit" => Ok(Self::Fit),
            "stretch" => Ok(Self::Stretch),
            "tile" => Ok(Self::Tile),
            "center" => Ok(Self::Center),
            "span" => Ok(Self::Span),
            other => Err(ConfigError::UnknownFit(other.to_string())),
        }
    }
}

// ============================================================================
// DynamicTheme -- time-of-day color palette
// ============================================================================

/// Number of time-of-day phases in the dynamic theme.
const PHASE_COUNT: usize = 5;

/// Time-of-day color palette for dynamic wallpaper mode.
///
/// Each phase defines a background color; the renderer smoothly interpolates
/// between adjacent phases based on the current time of day.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicTheme {
    /// Dawn colors (around 05:00-07:00).
    pub dawn: Color,
    /// Morning colors (around 08:00-11:00).
    pub morning: Color,
    /// Afternoon colors (around 12:00-16:00).
    pub afternoon: Color,
    /// Evening/sunset colors (around 17:00-19:00).
    pub evening: Color,
    /// Night colors (around 20:00-04:00).
    pub night: Color,
}

impl Default for DynamicTheme {
    fn default() -> Self {
        Self {
            dawn: Color::from_hex(0x2E1A47),      // deep purple-blue
            morning: Color::from_hex(0x1A3A5C),    // cool blue
            afternoon: Color::from_hex(0x1E4D6E),  // warm teal-blue
            evening: Color::from_hex(0x4A2040),     // warm purple-red
            night: Color::from_hex(0x0D0D1A),       // near-black blue
        }
    }
}

impl DynamicTheme {
    /// Create a dynamic theme from a base palette of five colors.
    pub fn from_palette(colors: [Color; PHASE_COUNT]) -> Self {
        Self {
            dawn: colors[0],
            morning: colors[1],
            afternoon: colors[2],
            evening: colors[3],
            night: colors[4],
        }
    }

    /// Phase boundaries in hours (24h clock). Each element is the start hour
    /// of that phase.
    ///
    /// Layout:  night [0..5)  dawn [5..8)  morning [8..12)
    ///          afternoon [12..17)  evening [17..20)  night [20..24)
    const PHASE_HOURS: [f32; PHASE_COUNT] = [5.0, 8.0, 12.0, 17.0, 20.0];

    /// Duration of the transition ramp in hours.
    const TRANSITION_HOURS: f32 = 1.5;

    /// Compute the background color for a given time of day.
    ///
    /// `time_secs` is seconds since midnight (0..86400). Values outside this
    /// range are wrapped.
    pub fn color_at(&self, time_secs: u64) -> Color {
        let secs_in_day = (time_secs % 86400) as f32;
        let hour = secs_in_day / 3600.0;

        let phases = [self.dawn, self.morning, self.afternoon, self.evening, self.night];
        let starts = Self::PHASE_HOURS;

        // Determine which phase we are in or transitioning between.
        // The order wraps: night < dawn < morning < afternoon < evening < night.
        // Index 0=dawn, 1=morning, 2=afternoon, 3=evening, 4=night.
        // Before dawn (hour < 5) or after evening end (hour >= 20) is night.

        // Find the current phase index.
        let phase_idx = if hour < starts[0] {
            // Before dawn -- night phase.
            4
        } else {
            let mut idx = 4usize;
            for (i, &start) in starts.iter().enumerate() {
                if hour >= start {
                    idx = i;
                }
            }
            idx
        };

        let phase_start = starts[phase_idx];
        let next_idx = (phase_idx + 1) % PHASE_COUNT;
        let next_start = if next_idx == 0 {
            // Wrapping from night back to dawn. Dawn starts at hour 5,
            // so from the perspective of hour >= 20, that is 24 + 5 = 29.
            starts[0] + 24.0
        } else {
            starts[next_idx]
        };

        // How far into this phase are we (normalized)?
        let phase_duration = next_start - phase_start;
        let adjusted_hour = if hour < phase_start {
            hour + 24.0
        } else {
            hour
        };
        let elapsed = adjusted_hour - phase_start;

        // Transition occupies the last TRANSITION_HOURS of each phase.
        let transition_start = phase_duration - Self::TRANSITION_HOURS;
        if elapsed >= transition_start && phase_duration > 0.0 {
            let t = ((elapsed - transition_start) / Self::TRANSITION_HOURS).clamp(0.0, 1.0);
            phases[phase_idx].lerp(phases[next_idx], t)
        } else {
            phases[phase_idx]
        }
    }
}

// ============================================================================
// SlideshowState
// ============================================================================

/// Maximum entries tracked for wallpaper history.
const HISTORY_CAPACITY: usize = 20;

/// Runtime state for slideshow mode.
#[derive(Clone, Debug)]
pub struct SlideshowState {
    /// Ordered list of image paths in the slideshow directory.
    pub paths: Vec<String>,
    /// Current index into `paths` (or `shuffle_order` if shuffled).
    pub current_index: usize,
    /// Permuted indices when shuffle is enabled.
    pub shuffle_order: Vec<usize>,
    /// Timestamp (seconds) when the last image change occurred.
    pub last_change_secs: u64,
}

impl SlideshowState {
    /// Create a new slideshow state from a list of image paths.
    pub fn new(paths: Vec<String>) -> Self {
        let len = paths.len();
        Self {
            paths,
            current_index: 0,
            shuffle_order: (0..len).collect(),
            last_change_secs: 0,
        }
    }

    /// The effective index into `paths` accounting for shuffle order.
    pub fn effective_index(&self) -> Option<usize> {
        if self.paths.is_empty() {
            return None;
        }
        let pos = self.current_index % self.shuffle_order.len().max(1);
        self.shuffle_order.get(pos).copied()
    }

    /// Current image path, if any.
    pub fn current_path(&self) -> Option<&str> {
        let idx = self.effective_index()?;
        self.paths.get(idx).map(String::as_str)
    }

    /// Advance to the next image. Returns `true` if the image changed.
    pub fn advance(&mut self) -> bool {
        if self.paths.is_empty() {
            return false;
        }
        let max = self.shuffle_order.len().max(1);
        self.current_index = (self.current_index + 1) % max;
        true
    }

    /// Go back to the previous image. Returns `true` if the image changed.
    pub fn go_back(&mut self) -> bool {
        if self.paths.is_empty() {
            return false;
        }
        let max = self.shuffle_order.len().max(1);
        if self.current_index == 0 {
            self.current_index = max.saturating_sub(1);
        } else {
            self.current_index -= 1;
        }
        true
    }

    /// Generate a deterministic shuffle order from a seed value.
    ///
    /// Uses a simple Fisher-Yates style permutation with a linear
    /// congruential generator for reproducibility without pulling in
    /// a full RNG crate.
    pub fn shuffle_with_seed(&mut self, seed: u64) {
        let n = self.paths.len();
        self.shuffle_order = (0..n).collect();

        if n <= 1 {
            return;
        }

        // Simple LCG: state = (a * state + c) mod m.
        let mut rng_state = seed.wrapping_add(1);
        for i in (1..n).rev() {
            rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let j = (rng_state >> 33) as usize % (i + 1);
            self.shuffle_order.swap(i, j);
        }
    }
}

// ============================================================================
// WallpaperHistory
// ============================================================================

/// Tracks recent wallpapers for back-navigation.
#[derive(Clone, Debug)]
pub struct WallpaperHistory {
    /// Ring buffer of recent wallpaper descriptions (path or color hex).
    entries: Vec<String>,
    /// Index of the "current" entry in the history for back/forward.
    position: usize,
}

impl WallpaperHistory {
    /// Create an empty history.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            position: 0,
        }
    }

    /// Record a new wallpaper. Truncates any forward history.
    pub fn push(&mut self, entry: String) {
        // If we are not at the end (user went back then changed),
        // truncate the forward entries.
        if self.position < self.entries.len() {
            self.entries.truncate(self.position);
        }

        self.entries.push(entry);

        // Enforce capacity limit.
        if self.entries.len() > HISTORY_CAPACITY {
            let excess = self.entries.len() - HISTORY_CAPACITY;
            self.entries.drain(..excess);
        }
        self.position = self.entries.len();
    }

    /// Navigate back. Returns the previous entry, if any.
    pub fn go_back(&mut self) -> Option<&str> {
        if self.position > 1 {
            self.position -= 1;
            self.entries.get(self.position - 1).map(String::as_str)
        } else {
            None
        }
    }

    /// Navigate forward. Returns the next entry, if any.
    pub fn go_forward(&mut self) -> Option<&str> {
        if self.position < self.entries.len() {
            self.position += 1;
            self.entries.get(self.position - 1).map(String::as_str)
        } else {
            None
        }
    }

    /// Current entry, if any.
    pub fn current(&self) -> Option<&str> {
        if self.position > 0 && self.position <= self.entries.len() {
            self.entries.get(self.position - 1).map(String::as_str)
        } else {
            None
        }
    }

    /// Number of entries in the history.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the history is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for WallpaperHistory {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// WallpaperConfig
// ============================================================================

/// Complete wallpaper configuration.
#[derive(Clone, Debug)]
pub struct WallpaperConfig {
    /// Active mode.
    pub mode: WallpaperMode,
    /// Path to the current single image (for SingleImage mode).
    pub image_path: String,
    /// Solid background color.
    pub color: Color,
    /// Slideshow directory path.
    pub slideshow_dir: String,
    /// Slideshow interval in seconds.
    pub slideshow_interval_secs: u64,
    /// Whether slideshow order is shuffled.
    pub slideshow_shuffle: bool,
    /// Image fitting mode.
    pub fit: ImageFit,
    /// Dynamic theme palette.
    pub dynamic_theme: DynamicTheme,
}

impl Default for WallpaperConfig {
    fn default() -> Self {
        Self {
            mode: WallpaperMode::SolidColor,
            image_path: String::new(),
            color: palette::BASE,
            slideshow_dir: String::new(),
            slideshow_interval_secs: 300,
            slideshow_shuffle: false,
            fit: ImageFit::Fill,
            dynamic_theme: DynamicTheme::default(),
        }
    }
}

// ============================================================================
// WallpaperManager
// ============================================================================

/// Manages the desktop wallpaper lifecycle: configuration, state,
/// slideshow timing, history, and render command generation.
pub struct WallpaperManager {
    /// Current configuration.
    pub config: WallpaperConfig,
    /// Slideshow runtime state (populated when mode is Slideshow).
    pub slideshow: Option<SlideshowState>,
    /// Recent wallpaper history for back/forward navigation.
    pub history: WallpaperHistory,
    /// Image ID for the current wallpaper image (opaque handle for the
    /// renderer). Zero means no image loaded.
    current_image_id: u64,
    /// Monotonic counter for generating image IDs.
    next_image_id: u64,
}

impl WallpaperManager {
    /// Create a new wallpaper manager with default settings (solid color).
    pub fn new() -> Self {
        Self {
            config: WallpaperConfig::default(),
            slideshow: None,
            history: WallpaperHistory::new(),
            current_image_id: 0,
            next_image_id: 1,
        }
    }

    // ======================================================================
    // Mode setters
    // ======================================================================

    /// Set the wallpaper to a solid color.
    pub fn set_solid_color(&mut self, color: Color) {
        self.config.mode = WallpaperMode::SolidColor;
        self.config.color = color;
        self.slideshow = None;
        self.current_image_id = 0;
        self.history.push(format!(
            "solid:#{:02X}{:02X}{:02X}",
            color.r, color.g, color.b
        ));
    }

    /// Set the wallpaper to a single image.
    pub fn set_image(&mut self, path: &str, fit: ImageFit) {
        self.config.mode = WallpaperMode::SingleImage;
        self.config.image_path = path.to_string();
        self.config.fit = fit;
        self.slideshow = None;
        self.current_image_id = self.alloc_image_id();
        self.history.push(format!("image:{path}"));
    }

    /// Set the wallpaper to slideshow mode.
    ///
    /// `directory` is the path containing images. `interval_secs` is the
    /// time between transitions. `shuffle` randomises the order.
    ///
    /// The caller is responsible for populating image paths via
    /// [`populate_slideshow_paths`](Self::populate_slideshow_paths) after
    /// calling this, since the wallpaper manager does not perform I/O.
    pub fn set_slideshow(&mut self, directory: &str, interval_secs: u64, shuffle: bool) {
        self.config.mode = WallpaperMode::Slideshow;
        self.config.slideshow_dir = directory.to_string();
        self.config.slideshow_interval_secs = interval_secs.max(1);
        self.config.slideshow_shuffle = shuffle;
        // Start with an empty slideshow -- the caller populates paths.
        self.slideshow = Some(SlideshowState::new(Vec::new()));
        self.current_image_id = 0;
    }

    /// Populate the slideshow state with a list of image paths.
    ///
    /// This is separated from `set_slideshow` because the wallpaper manager
    /// itself does not perform filesystem I/O -- the desktop shell scans
    /// the directory and passes the results here.
    pub fn populate_slideshow_paths(&mut self, paths: Vec<String>, seed: u64) {
        let mut state = SlideshowState::new(paths);
        if self.config.slideshow_shuffle {
            state.shuffle_with_seed(seed);
        }
        if let Some(path) = state.current_path() {
            self.current_image_id = self.alloc_image_id();
            self.history.push(format!("slideshow:{path}"));
        }
        self.slideshow = Some(state);
    }

    /// Set the wallpaper to dynamic time-of-day mode.
    pub fn set_dynamic_theme(&mut self, base_palette: [Color; PHASE_COUNT]) {
        self.config.mode = WallpaperMode::Dynamic;
        self.config.dynamic_theme = DynamicTheme::from_palette(base_palette);
        self.slideshow = None;
        self.current_image_id = 0;
        self.history.push("dynamic".to_string());
    }

    // ======================================================================
    // Tick / timing
    // ======================================================================

    /// Advance the wallpaper state. Call once per frame (or per second).
    ///
    /// `current_time_secs` is seconds since midnight for dynamic mode,
    /// or a monotonic timestamp for slideshow timing.
    ///
    /// Returns `true` if the wallpaper visually changed (slideshow advanced
    /// or dynamic color shifted enough to warrant a redraw).
    pub fn tick(&mut self, current_time_secs: u64) -> bool {
        match self.config.mode {
            WallpaperMode::Slideshow => self.tick_slideshow(current_time_secs),
            WallpaperMode::Dynamic => {
                // Dynamic mode always changes (smooth gradient), but we
                // signal a meaningful change only when crossing a phase
                // boundary. For simplicity, return true every call since
                // the color is time-dependent.
                true
            }
            WallpaperMode::SolidColor | WallpaperMode::SingleImage => false,
        }
    }

    /// Internal slideshow tick logic.
    fn tick_slideshow(&mut self, current_time_secs: u64) -> bool {
        let interval = self.config.slideshow_interval_secs;

        // Perform all slideshow state mutations first, then update
        // manager-level fields afterward to satisfy the borrow checker.
        let advanced = {
            let Some(ref mut state) = self.slideshow else {
                return false;
            };

            if state.paths.is_empty() {
                return false;
            }

            // Initialize timestamp on first tick.
            if state.last_change_secs == 0 {
                state.last_change_secs = current_time_secs;
                return false;
            }

            let elapsed = current_time_secs.saturating_sub(state.last_change_secs);
            if elapsed >= interval {
                state.last_change_secs = current_time_secs;
                state.advance()
            } else {
                false
            }
        };

        if advanced {
            self.current_image_id = self.alloc_image_id();
            if let Some(path) = self.slideshow.as_ref().and_then(|s| s.current_path()) {
                self.history.push(format!("slideshow:{path}"));
            }
        }

        advanced
    }

    // ======================================================================
    // Manual slideshow controls
    // ======================================================================

    /// Advance to the next wallpaper in the slideshow.
    pub fn next_wallpaper(&mut self) {
        let advanced = self.slideshow.as_mut().is_some_and(|s| s.advance());
        if advanced {
            self.current_image_id = self.alloc_image_id();
            if let Some(path) = self.slideshow.as_ref().and_then(|s| s.current_path()) {
                self.history.push(format!("slideshow:{path}"));
            }
            if let Some(ref mut s) = self.slideshow {
                s.last_change_secs = 0; // Reset timer.
            }
        }
    }

    /// Go back to the previous wallpaper in the slideshow.
    pub fn previous_wallpaper(&mut self) {
        let went_back = self.slideshow.as_mut().is_some_and(|s| s.go_back());
        if went_back {
            self.current_image_id = self.alloc_image_id();
            if let Some(path) = self.slideshow.as_ref().and_then(|s| s.current_path()) {
                self.history.push(format!("slideshow:{path}"));
            }
            if let Some(ref mut s) = self.slideshow {
                s.last_change_secs = 0;
            }
        }
    }

    /// Jump to a random wallpaper in the slideshow.
    ///
    /// `seed` is used as the entropy source since the wallpaper manager
    /// does not import an RNG crate.
    pub fn random_wallpaper(&mut self, seed: u64) {
        // Compute the random index and effective path index, then update
        // the slideshow state -- all in one scope to release the borrow
        // before touching other fields.
        let effective_path = {
            let Some(ref mut state) = self.slideshow else {
                return;
            };
            if state.paths.is_empty() {
                return;
            }
            // Simple hash of the seed to pick an index.
            let mixed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let idx = (mixed >> 33) as usize % state.paths.len();
            let effective = if idx < state.shuffle_order.len() {
                state.shuffle_order[idx]
            } else {
                idx % state.paths.len()
            };
            state.current_index = idx;
            state.last_change_secs = 0;
            state.paths.get(effective).cloned()
        };

        self.current_image_id = self.alloc_image_id();
        if let Some(path) = effective_path {
            self.history.push(format!("slideshow:{path}"));
        }
    }

    // ======================================================================
    // Rendering
    // ======================================================================

    /// Produce render commands for the desktop background.
    ///
    /// The commands fill the area `(0, 0, width, height)` with the
    /// appropriate background for the current mode.
    ///
    /// For image modes, an `Image` render command is emitted referencing
    /// `current_image_id`. The compositor is responsible for mapping that
    /// ID to actual pixel data.
    ///
    /// `time_secs` is seconds since midnight, used only for Dynamic mode.
    pub fn get_render_commands(
        &self,
        width: f32,
        height: f32,
        time_secs: u64,
    ) -> Vec<RenderCommand> {
        match self.config.mode {
            WallpaperMode::SolidColor => self.render_solid(width, height),
            WallpaperMode::SingleImage => self.render_image(width, height),
            WallpaperMode::Slideshow => self.render_slideshow(width, height),
            WallpaperMode::Dynamic => self.render_dynamic(width, height, time_secs),
        }
    }

    /// Render a solid color background.
    fn render_solid(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        vec![RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: self.config.color,
            corner_radii: CornerRadii::ZERO,
        }]
    }

    /// Render a single image background with a color underlay.
    fn render_image(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(2);

        // Background color underlay (visible through letterboxing or if
        // image fails to load).
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: self.config.color,
            corner_radii: CornerRadii::ZERO,
        });

        if self.current_image_id != 0 {
            let (ix, iy, iw, ih) = compute_image_rect(width, height, width, height, self.config.fit);
            cmds.push(RenderCommand::Image {
                x: ix,
                y: iy,
                width: iw,
                height: ih,
                image_id: self.current_image_id,
            });
        }

        cmds
    }

    /// Render the current slideshow image.
    fn render_slideshow(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        // Same rendering as single image -- the slideshow logic only
        // changes which image_id is current.
        self.render_image(width, height)
    }

    /// Render a dynamic time-of-day gradient background.
    ///
    /// We approximate a vertical gradient by drawing horizontal strips.
    /// The top color is the base phase color; the bottom color is blended
    /// with a darker variant to create depth.
    fn render_dynamic(&self, width: f32, height: f32, time_secs: u64) -> Vec<RenderCommand> {
        let base_color = self.config.dynamic_theme.color_at(time_secs);

        // Create a subtle gradient: top is the phase color, bottom is
        // slightly darker. We render GRADIENT_STRIPS horizontal bands.
        const GRADIENT_STRIPS: usize = 16;
        let mut cmds = Vec::with_capacity(GRADIENT_STRIPS);

        let strip_height = height / GRADIENT_STRIPS as f32;
        let dark = Color::rgba(
            base_color.r.saturating_sub(20),
            base_color.g.saturating_sub(20),
            base_color.b.saturating_sub(20),
            base_color.a,
        );

        for i in 0..GRADIENT_STRIPS {
            let t = i as f32 / (GRADIENT_STRIPS - 1).max(1) as f32;
            let color = base_color.lerp(dark, t);
            cmds.push(RenderCommand::FillRect {
                x: 0.0,
                y: strip_height * i as f32,
                width,
                height: strip_height + 1.0, // +1 to avoid gaps from rounding
                color,
                corner_radii: CornerRadii::ZERO,
            });
        }

        cmds
    }

    // ======================================================================
    // Configuration persistence
    // ======================================================================

    /// Serialise the current configuration to a key=value text format.
    pub fn save_config(&self) -> String {
        let mut out = String::new();

        out.push_str(&format!("mode={}\n", self.config.mode.as_str()));
        out.push_str(&format!("fit={}\n", self.config.fit.as_str()));
        out.push_str(&format!(
            "color={:02X}{:02X}{:02X}\n",
            self.config.color.r, self.config.color.g, self.config.color.b
        ));

        if !self.config.image_path.is_empty() {
            out.push_str(&format!("image_path={}\n", self.config.image_path));
        }

        if !self.config.slideshow_dir.is_empty() {
            out.push_str(&format!("slideshow_dir={}\n", self.config.slideshow_dir));
        }
        out.push_str(&format!(
            "slideshow_interval={}\n",
            self.config.slideshow_interval_secs
        ));
        out.push_str(&format!(
            "slideshow_shuffle={}\n",
            self.config.slideshow_shuffle
        ));

        // Dynamic theme colors.
        let dt = &self.config.dynamic_theme;
        out.push_str(&format!(
            "dynamic_dawn={:02X}{:02X}{:02X}\n",
            dt.dawn.r, dt.dawn.g, dt.dawn.b
        ));
        out.push_str(&format!(
            "dynamic_morning={:02X}{:02X}{:02X}\n",
            dt.morning.r, dt.morning.g, dt.morning.b
        ));
        out.push_str(&format!(
            "dynamic_afternoon={:02X}{:02X}{:02X}\n",
            dt.afternoon.r, dt.afternoon.g, dt.afternoon.b
        ));
        out.push_str(&format!(
            "dynamic_evening={:02X}{:02X}{:02X}\n",
            dt.evening.r, dt.evening.g, dt.evening.b
        ));
        out.push_str(&format!(
            "dynamic_night={:02X}{:02X}{:02X}\n",
            dt.night.r, dt.night.g, dt.night.b
        ));

        out
    }

    /// Load configuration from a key=value text string.
    pub fn load_config(text: &str) -> Result<WallpaperConfig, ConfigError> {
        let mut mode: Option<WallpaperMode> = None;
        let mut fit: Option<ImageFit> = None;
        let mut color: Option<Color> = None;
        let mut image_path = String::new();
        let mut slideshow_dir = String::new();
        let mut slideshow_interval: Option<u64> = None;
        let mut slideshow_shuffle: Option<bool> = None;
        let mut dt_dawn: Option<Color> = None;
        let mut dt_morning: Option<Color> = None;
        let mut dt_afternoon: Option<Color> = None;
        let mut dt_evening: Option<Color> = None;
        let mut dt_night: Option<Color> = None;

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let Some((key, val)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let val = val.trim();

            match key {
                "mode" => mode = Some(WallpaperMode::from_str_config(val)?),
                "fit" => fit = Some(ImageFit::from_str_config(val)?),
                "color" => color = Some(parse_hex_color(val)?),
                "image_path" => image_path = val.to_string(),
                "slideshow_dir" => slideshow_dir = val.to_string(),
                "slideshow_interval" => {
                    slideshow_interval = Some(val.parse::<u64>().map_err(|_| {
                        ConfigError::InvalidValue(format!("slideshow_interval: {val}"))
                    })?);
                }
                "slideshow_shuffle" => {
                    slideshow_shuffle = Some(val.parse::<bool>().map_err(|_| {
                        ConfigError::InvalidValue(format!("slideshow_shuffle: {val}"))
                    })?);
                }
                "dynamic_dawn" => dt_dawn = Some(parse_hex_color(val)?),
                "dynamic_morning" => dt_morning = Some(parse_hex_color(val)?),
                "dynamic_afternoon" => dt_afternoon = Some(parse_hex_color(val)?),
                "dynamic_evening" => dt_evening = Some(parse_hex_color(val)?),
                "dynamic_night" => dt_night = Some(parse_hex_color(val)?),
                _ => {
                    // Ignore unknown keys for forward compatibility.
                }
            }
        }

        let mode = mode.ok_or_else(|| ConfigError::MissingKey("mode".into()))?;

        let default_dt = DynamicTheme::default();
        let dynamic_theme = DynamicTheme {
            dawn: dt_dawn.unwrap_or(default_dt.dawn),
            morning: dt_morning.unwrap_or(default_dt.morning),
            afternoon: dt_afternoon.unwrap_or(default_dt.afternoon),
            evening: dt_evening.unwrap_or(default_dt.evening),
            night: dt_night.unwrap_or(default_dt.night),
        };

        Ok(WallpaperConfig {
            mode,
            image_path,
            color: color.unwrap_or(palette::BASE),
            slideshow_dir,
            slideshow_interval_secs: slideshow_interval.unwrap_or(300),
            slideshow_shuffle: slideshow_shuffle.unwrap_or(false),
            fit: fit.unwrap_or(ImageFit::Fill),
            dynamic_theme,
        })
    }

    /// Replace the current config and reinitialise state accordingly.
    pub fn apply_config(&mut self, config: WallpaperConfig) {
        self.config = config;
        self.slideshow = None;
        self.current_image_id = 0;

        match self.config.mode {
            WallpaperMode::SingleImage => {
                if !self.config.image_path.is_empty() {
                    self.current_image_id = self.alloc_image_id();
                }
            }
            WallpaperMode::Slideshow => {
                self.slideshow = Some(SlideshowState::new(Vec::new()));
            }
            WallpaperMode::SolidColor | WallpaperMode::Dynamic => {}
        }
    }

    // ======================================================================
    // Accessors
    // ======================================================================

    /// The current wallpaper mode.
    pub fn mode(&self) -> &WallpaperMode {
        &self.config.mode
    }

    /// The image ID that the compositor should use for the current image.
    pub fn current_image_id(&self) -> u64 {
        self.current_image_id
    }

    /// The path of the current wallpaper image, if applicable.
    pub fn current_image_path(&self) -> Option<&str> {
        match self.config.mode {
            WallpaperMode::SingleImage => {
                if self.config.image_path.is_empty() {
                    None
                } else {
                    Some(&self.config.image_path)
                }
            }
            WallpaperMode::Slideshow => {
                self.slideshow.as_ref().and_then(|s| s.current_path())
            }
            _ => None,
        }
    }

    // ======================================================================
    // Internal helpers
    // ======================================================================

    /// Allocate a new unique image ID.
    fn alloc_image_id(&mut self) -> u64 {
        let id = self.next_image_id;
        self.next_image_id = self.next_image_id.wrapping_add(1);
        if self.next_image_id == 0 {
            // Skip zero (reserved for "no image").
            self.next_image_id = 1;
        }
        id
    }
}

impl Default for WallpaperManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Parse a 6-character hex color string (e.g., "1E1E2E") into a Color.
fn parse_hex_color(s: &str) -> Result<Color, ConfigError> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return Err(ConfigError::InvalidValue(format!("color: {s} (expected 6 hex digits)")));
    }
    let val = u32::from_str_radix(s, 16)
        .map_err(|_| ConfigError::InvalidValue(format!("color: {s}")))?;
    Ok(Color::from_hex(val))
}

/// Compute the destination rectangle for an image given the display area,
/// the image's native size, and the desired fit mode.
///
/// Returns `(x, y, width, height)` in display coordinates.
fn compute_image_rect(
    display_w: f32,
    display_h: f32,
    image_w: f32,
    image_h: f32,
    fit: ImageFit,
) -> (f32, f32, f32, f32) {
    if display_w <= 0.0 || display_h <= 0.0 || image_w <= 0.0 || image_h <= 0.0 {
        return (0.0, 0.0, display_w, display_h);
    }

    match fit {
        ImageFit::Stretch => (0.0, 0.0, display_w, display_h),

        ImageFit::Fill => {
            let scale = (display_w / image_w).max(display_h / image_h);
            let w = image_w * scale;
            let h = image_h * scale;
            let x = (display_w - w) / 2.0;
            let y = (display_h - h) / 2.0;
            (x, y, w, h)
        }

        ImageFit::Fit => {
            let scale = (display_w / image_w).min(display_h / image_h);
            let w = image_w * scale;
            let h = image_h * scale;
            let x = (display_w - w) / 2.0;
            let y = (display_h - h) / 2.0;
            (x, y, w, h)
        }

        ImageFit::Center => {
            let x = (display_w - image_w) / 2.0;
            let y = (display_h - image_h) / 2.0;
            (x, y, image_w, image_h)
        }

        ImageFit::Tile => {
            // For tile mode, we position the first tile at the origin.
            // The compositor handles repeating. We report a single tile.
            (0.0, 0.0, image_w, image_h)
        }

        ImageFit::Span => {
            // Span mode is identical to Stretch for a single monitor.
            // Multi-monitor spanning is handled at a higher level by
            // passing the total virtual desktop dimensions.
            (0.0, 0.0, display_w, display_h)
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // WallpaperMode parsing
    // ------------------------------------------------------------------

    #[test]
    fn mode_roundtrip_solid() {
        let mode = WallpaperMode::SolidColor;
        let parsed = WallpaperMode::from_str_config(mode.as_str()).expect("parse");
        assert_eq!(parsed, mode);
    }

    #[test]
    fn mode_roundtrip_all() {
        for mode in [
            WallpaperMode::SolidColor,
            WallpaperMode::SingleImage,
            WallpaperMode::Slideshow,
            WallpaperMode::Dynamic,
        ] {
            let parsed =
                WallpaperMode::from_str_config(mode.as_str()).expect("should parse");
            assert_eq!(parsed, mode);
        }
    }

    #[test]
    fn mode_unknown_returns_error() {
        let result = WallpaperMode::from_str_config("bogus");
        assert!(matches!(result, Err(ConfigError::UnknownMode(_))));
    }

    // ------------------------------------------------------------------
    // ImageFit parsing
    // ------------------------------------------------------------------

    #[test]
    fn fit_roundtrip_all() {
        for fit in [
            ImageFit::Fill,
            ImageFit::Fit,
            ImageFit::Stretch,
            ImageFit::Tile,
            ImageFit::Center,
            ImageFit::Span,
        ] {
            let parsed =
                ImageFit::from_str_config(fit.as_str()).expect("should parse");
            assert_eq!(parsed, fit);
        }
    }

    #[test]
    fn fit_unknown_returns_error() {
        let result = ImageFit::from_str_config("unknown");
        assert!(matches!(result, Err(ConfigError::UnknownFit(_))));
    }

    // ------------------------------------------------------------------
    // DynamicTheme color interpolation
    // ------------------------------------------------------------------

    #[test]
    fn dynamic_theme_night_at_midnight() {
        let theme = DynamicTheme::default();
        let color = theme.color_at(0); // midnight
        // Should be the night color (possibly transitioning toward dawn,
        // but at midnight we are deep in night phase).
        assert_eq!(color, theme.night);
    }

    #[test]
    fn dynamic_theme_dawn_at_6am() {
        let theme = DynamicTheme::default();
        // 6:00 AM = 6 * 3600 = 21600 seconds.
        let color = theme.color_at(21600);
        // Should be within the dawn phase (dawn runs 5..8).
        // At 6am, we are in the stable part of dawn (before transition).
        assert_eq!(color, theme.dawn);
    }

    #[test]
    fn dynamic_theme_afternoon_at_1pm() {
        let theme = DynamicTheme::default();
        // 1:00 PM = 13 * 3600 = 46800 seconds.
        let color = theme.color_at(46800);
        // Should be the afternoon color (stable part, 12..15.5).
        assert_eq!(color, theme.afternoon);
    }

    #[test]
    fn dynamic_theme_wraps_at_86400() {
        let theme = DynamicTheme::default();
        let color_at_0 = theme.color_at(0);
        let color_at_86400 = theme.color_at(86400);
        assert_eq!(color_at_0, color_at_86400);
    }

    #[test]
    fn dynamic_theme_from_palette() {
        let colors = [
            Color::rgb(10, 20, 30),
            Color::rgb(40, 50, 60),
            Color::rgb(70, 80, 90),
            Color::rgb(100, 110, 120),
            Color::rgb(130, 140, 150),
        ];
        let theme = DynamicTheme::from_palette(colors);
        assert_eq!(theme.dawn, colors[0]);
        assert_eq!(theme.morning, colors[1]);
        assert_eq!(theme.afternoon, colors[2]);
        assert_eq!(theme.evening, colors[3]);
        assert_eq!(theme.night, colors[4]);
    }

    // ------------------------------------------------------------------
    // SlideshowState
    // ------------------------------------------------------------------

    #[test]
    fn slideshow_empty_paths() {
        let state = SlideshowState::new(Vec::new());
        assert!(state.current_path().is_none());
        assert!(state.effective_index().is_none());
    }

    #[test]
    fn slideshow_single_path() {
        let state = SlideshowState::new(vec!["a.png".into()]);
        assert_eq!(state.current_path(), Some("a.png"));
    }

    #[test]
    fn slideshow_advance_wraps() {
        let mut state = SlideshowState::new(vec!["a.png".into(), "b.png".into()]);
        assert_eq!(state.current_path(), Some("a.png"));
        assert!(state.advance());
        assert_eq!(state.current_path(), Some("b.png"));
        assert!(state.advance());
        assert_eq!(state.current_path(), Some("a.png")); // wrapped
    }

    #[test]
    fn slideshow_go_back_wraps() {
        let mut state = SlideshowState::new(vec!["a.png".into(), "b.png".into(), "c.png".into()]);
        assert_eq!(state.current_index, 0);
        assert!(state.go_back()); // wraps to last
        assert_eq!(state.current_index, 2);
        assert_eq!(state.current_path(), Some("c.png"));
    }

    #[test]
    fn slideshow_shuffle_changes_order() {
        let mut state = SlideshowState::new(vec![
            "a.png".into(),
            "b.png".into(),
            "c.png".into(),
            "d.png".into(),
            "e.png".into(),
        ]);
        let original_order = state.shuffle_order.clone();
        state.shuffle_with_seed(42);
        // Shuffled order should differ from sequential (with high probability
        // for 5 elements and a non-trivial seed).
        assert_ne!(state.shuffle_order, original_order);
        // All indices should still be present.
        let mut sorted = state.shuffle_order.clone();
        sorted.sort();
        assert_eq!(sorted, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn slideshow_shuffle_single_element() {
        let mut state = SlideshowState::new(vec!["only.png".into()]);
        state.shuffle_with_seed(99);
        assert_eq!(state.shuffle_order, vec![0]);
        assert_eq!(state.current_path(), Some("only.png"));
    }

    #[test]
    fn slideshow_advance_empty_returns_false() {
        let mut state = SlideshowState::new(Vec::new());
        assert!(!state.advance());
    }

    // ------------------------------------------------------------------
    // WallpaperHistory
    // ------------------------------------------------------------------

    #[test]
    fn history_empty() {
        let history = WallpaperHistory::new();
        assert!(history.is_empty());
        assert_eq!(history.len(), 0);
        assert!(history.current().is_none());
    }

    #[test]
    fn history_push_and_current() {
        let mut history = WallpaperHistory::new();
        history.push("a.png".into());
        assert_eq!(history.current(), Some("a.png"));
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn history_go_back_and_forward() {
        let mut history = WallpaperHistory::new();
        history.push("first.png".into());
        history.push("second.png".into());
        history.push("third.png".into());

        assert_eq!(history.current(), Some("third.png"));

        let back = history.go_back();
        assert_eq!(back, Some("second.png"));
        assert_eq!(history.current(), Some("second.png"));

        let forward = history.go_forward();
        assert_eq!(forward, Some("third.png"));
    }

    #[test]
    fn history_go_back_at_start_returns_none() {
        let mut history = WallpaperHistory::new();
        history.push("only.png".into());
        assert!(history.go_back().is_none());
    }

    #[test]
    fn history_go_forward_at_end_returns_none() {
        let mut history = WallpaperHistory::new();
        history.push("only.png".into());
        assert!(history.go_forward().is_none());
    }

    #[test]
    fn history_push_after_back_truncates_forward() {
        let mut history = WallpaperHistory::new();
        history.push("a.png".into());
        history.push("b.png".into());
        history.push("c.png".into());

        // Go back to "b"
        history.go_back();
        assert_eq!(history.current(), Some("b.png"));

        // Push a new entry -- "c" should be gone
        history.push("d.png".into());
        assert_eq!(history.current(), Some("d.png"));
        assert!(history.go_forward().is_none());
        assert_eq!(history.len(), 3); // a, b, d
    }

    #[test]
    fn history_respects_capacity() {
        let mut history = WallpaperHistory::new();
        for i in 0..30 {
            history.push(format!("wp_{i}.png"));
        }
        assert_eq!(history.len(), HISTORY_CAPACITY);
        // The oldest entries should have been evicted.
        assert_eq!(history.entries[0], "wp_10.png");
    }

    // ------------------------------------------------------------------
    // WallpaperManager -- mode setters
    // ------------------------------------------------------------------

    #[test]
    fn manager_default_is_solid() {
        let mgr = WallpaperManager::new();
        assert_eq!(*mgr.mode(), WallpaperMode::SolidColor);
        assert_eq!(mgr.current_image_id(), 0);
    }

    #[test]
    fn manager_set_solid_color() {
        let mut mgr = WallpaperManager::new();
        mgr.set_solid_color(Color::RED);
        assert_eq!(*mgr.mode(), WallpaperMode::SolidColor);
        assert_eq!(mgr.config.color, Color::RED);
        assert!(!mgr.history.is_empty());
    }

    #[test]
    fn manager_set_image() {
        let mut mgr = WallpaperManager::new();
        mgr.set_image("/wallpapers/sunset.png", ImageFit::Fill);
        assert_eq!(*mgr.mode(), WallpaperMode::SingleImage);
        assert_eq!(mgr.config.image_path, "/wallpapers/sunset.png");
        assert_eq!(mgr.config.fit, ImageFit::Fill);
        assert_ne!(mgr.current_image_id(), 0);
    }

    #[test]
    fn manager_set_slideshow() {
        let mut mgr = WallpaperManager::new();
        mgr.set_slideshow("/wallpapers", 60, true);
        assert_eq!(*mgr.mode(), WallpaperMode::Slideshow);
        assert_eq!(mgr.config.slideshow_dir, "/wallpapers");
        assert_eq!(mgr.config.slideshow_interval_secs, 60);
        assert!(mgr.config.slideshow_shuffle);
        assert!(mgr.slideshow.is_some());
    }

    #[test]
    fn manager_set_dynamic() {
        let mut mgr = WallpaperManager::new();
        let colors = [Color::RED, Color::GREEN, Color::BLUE, Color::WHITE, Color::BLACK];
        mgr.set_dynamic_theme(colors);
        assert_eq!(*mgr.mode(), WallpaperMode::Dynamic);
        assert_eq!(mgr.config.dynamic_theme.dawn, Color::RED);
    }

    // ------------------------------------------------------------------
    // Slideshow tick / advance
    // ------------------------------------------------------------------

    #[test]
    fn tick_slideshow_advances_on_interval() {
        let mut mgr = WallpaperManager::new();
        mgr.set_slideshow("/wp", 10, false);
        mgr.populate_slideshow_paths(
            vec!["a.png".into(), "b.png".into(), "c.png".into()],
            0,
        );

        // First tick initialises the timestamp.
        assert!(!mgr.tick(100));

        // Before interval passes -- no change.
        assert!(!mgr.tick(105));

        // After interval passes -- should advance.
        assert!(mgr.tick(111));
    }

    #[test]
    fn tick_solid_never_changes() {
        let mut mgr = WallpaperManager::new();
        mgr.set_solid_color(Color::BLUE);
        assert!(!mgr.tick(0));
        assert!(!mgr.tick(999999));
    }

    #[test]
    fn next_previous_wallpaper() {
        let mut mgr = WallpaperManager::new();
        mgr.set_slideshow("/wp", 300, false);
        mgr.populate_slideshow_paths(
            vec!["a.png".into(), "b.png".into(), "c.png".into()],
            0,
        );

        assert_eq!(mgr.current_image_path(), Some("a.png"));

        mgr.next_wallpaper();
        assert_eq!(mgr.current_image_path(), Some("b.png"));

        mgr.next_wallpaper();
        assert_eq!(mgr.current_image_path(), Some("c.png"));

        mgr.previous_wallpaper();
        assert_eq!(mgr.current_image_path(), Some("b.png"));
    }

    #[test]
    fn random_wallpaper_changes_image() {
        let mut mgr = WallpaperManager::new();
        mgr.set_slideshow("/wp", 300, false);
        mgr.populate_slideshow_paths(
            vec![
                "a.png".into(),
                "b.png".into(),
                "c.png".into(),
                "d.png".into(),
                "e.png".into(),
            ],
            0,
        );

        let id_before = mgr.current_image_id();
        mgr.random_wallpaper(42);
        // The image ID should have changed (new allocation).
        assert_ne!(mgr.current_image_id(), id_before);
    }

    // ------------------------------------------------------------------
    // Render commands
    // ------------------------------------------------------------------

    #[test]
    fn render_solid_produces_one_fill() {
        let mut mgr = WallpaperManager::new();
        mgr.set_solid_color(Color::from_hex(0x1E1E2E));
        let cmds = mgr.get_render_commands(1920.0, 1080.0, 0);
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            RenderCommand::FillRect { width, height, color, .. } => {
                assert!((width - 1920.0).abs() < f32::EPSILON);
                assert!((height - 1080.0).abs() < f32::EPSILON);
                assert_eq!(*color, Color::from_hex(0x1E1E2E));
            }
            other => panic!("expected FillRect, got {other:?}"),
        }
    }

    #[test]
    fn render_image_produces_fill_and_image() {
        let mut mgr = WallpaperManager::new();
        mgr.set_image("/test.png", ImageFit::Stretch);
        let cmds = mgr.get_render_commands(1920.0, 1080.0, 0);
        assert_eq!(cmds.len(), 2);
        assert!(matches!(&cmds[0], RenderCommand::FillRect { .. }));
        assert!(matches!(&cmds[1], RenderCommand::Image { .. }));
    }

    #[test]
    fn render_dynamic_produces_gradient_strips() {
        let mut mgr = WallpaperManager::new();
        mgr.set_dynamic_theme([Color::RED, Color::GREEN, Color::BLUE, Color::WHITE, Color::BLACK]);
        let cmds = mgr.get_render_commands(1920.0, 1080.0, 0);
        // Should produce 16 gradient strips.
        assert_eq!(cmds.len(), 16);
        for cmd in &cmds {
            assert!(matches!(cmd, RenderCommand::FillRect { .. }));
        }
    }

    #[test]
    fn render_slideshow_without_images_still_renders() {
        let mut mgr = WallpaperManager::new();
        mgr.set_slideshow("/empty", 300, false);
        // No paths populated -- should still render a background color.
        let cmds = mgr.get_render_commands(1920.0, 1080.0, 0);
        assert!(!cmds.is_empty());
        assert!(matches!(&cmds[0], RenderCommand::FillRect { .. }));
    }

    // ------------------------------------------------------------------
    // Config persistence round-trip
    // ------------------------------------------------------------------

    #[test]
    fn config_save_load_roundtrip_solid() {
        let mut mgr = WallpaperManager::new();
        mgr.set_solid_color(Color::from_hex(0x89B4FA));
        mgr.config.fit = ImageFit::Center;

        let text = mgr.save_config();
        let loaded = WallpaperManager::load_config(&text).expect("should parse");

        assert_eq!(loaded.mode, WallpaperMode::SolidColor);
        assert_eq!(loaded.color, Color::from_hex(0x89B4FA));
        assert_eq!(loaded.fit, ImageFit::Center);
    }

    #[test]
    fn config_save_load_roundtrip_slideshow() {
        let mut mgr = WallpaperManager::new();
        mgr.set_slideshow("/pictures/wallpapers", 120, true);
        mgr.config.fit = ImageFit::Fill;

        let text = mgr.save_config();
        let loaded = WallpaperManager::load_config(&text).expect("should parse");

        assert_eq!(loaded.mode, WallpaperMode::Slideshow);
        assert_eq!(loaded.slideshow_dir, "/pictures/wallpapers");
        assert_eq!(loaded.slideshow_interval_secs, 120);
        assert!(loaded.slideshow_shuffle);
        assert_eq!(loaded.fit, ImageFit::Fill);
    }

    #[test]
    fn config_load_missing_mode_errors() {
        let text = "fit=fill\ncolor=1E1E2E\n";
        let result = WallpaperManager::load_config(text);
        assert!(matches!(result, Err(ConfigError::MissingKey(_))));
    }

    #[test]
    fn config_load_unknown_mode_errors() {
        let text = "mode=holographic\n";
        let result = WallpaperManager::load_config(text);
        assert!(matches!(result, Err(ConfigError::UnknownMode(_))));
    }

    #[test]
    fn config_load_ignores_comments_and_blanks() {
        let text = "# Wallpaper config\nmode=solid\n\n# end\n";
        let loaded = WallpaperManager::load_config(text).expect("should parse");
        assert_eq!(loaded.mode, WallpaperMode::SolidColor);
    }

    #[test]
    fn config_dynamic_theme_roundtrip() {
        let mut mgr = WallpaperManager::new();
        let colors = [
            Color::rgb(10, 20, 30),
            Color::rgb(40, 50, 60),
            Color::rgb(70, 80, 90),
            Color::rgb(100, 110, 120),
            Color::rgb(5, 10, 15),
        ];
        mgr.set_dynamic_theme(colors);

        let text = mgr.save_config();
        let loaded = WallpaperManager::load_config(&text).expect("should parse");

        assert_eq!(loaded.dynamic_theme.dawn, colors[0]);
        assert_eq!(loaded.dynamic_theme.morning, colors[1]);
        assert_eq!(loaded.dynamic_theme.afternoon, colors[2]);
        assert_eq!(loaded.dynamic_theme.evening, colors[3]);
        assert_eq!(loaded.dynamic_theme.night, colors[4]);
    }

    // ------------------------------------------------------------------
    // compute_image_rect
    // ------------------------------------------------------------------

    #[test]
    fn image_rect_stretch() {
        let (x, y, w, h) = compute_image_rect(1920.0, 1080.0, 800.0, 600.0, ImageFit::Stretch);
        assert!((x).abs() < f32::EPSILON);
        assert!((y).abs() < f32::EPSILON);
        assert!((w - 1920.0).abs() < f32::EPSILON);
        assert!((h - 1080.0).abs() < f32::EPSILON);
    }

    #[test]
    fn image_rect_center() {
        let (x, y, w, h) = compute_image_rect(1920.0, 1080.0, 800.0, 600.0, ImageFit::Center);
        assert!((x - 560.0).abs() < f32::EPSILON); // (1920-800)/2
        assert!((y - 240.0).abs() < f32::EPSILON); // (1080-600)/2
        assert!((w - 800.0).abs() < f32::EPSILON);
        assert!((h - 600.0).abs() < f32::EPSILON);
    }

    #[test]
    fn image_rect_fill_wider_image() {
        // Image wider than display: scale by height, crop width.
        let (x, _y, w, h) = compute_image_rect(1920.0, 1080.0, 3000.0, 1000.0, ImageFit::Fill);
        // Scale = max(1920/3000, 1080/1000) = max(0.64, 1.08) = 1.08
        let expected_scale = 1080.0 / 1000.0;
        assert!((h - 1000.0 * expected_scale).abs() < 1.0);
        assert!((w - 3000.0 * expected_scale).abs() < 1.0);
        // Image should overflow horizontally (x < 0).
        assert!(x < 0.0);
    }

    #[test]
    fn image_rect_fit_wider_image() {
        // Image wider than display: scale by width, letterbox vertically.
        let (_x, y, w, h) = compute_image_rect(1920.0, 1080.0, 3000.0, 1000.0, ImageFit::Fit);
        // Scale = min(1920/3000, 1080/1000) = min(0.64, 1.08) = 0.64
        let expected_scale = 1920.0 / 3000.0;
        assert!((w - 3000.0 * expected_scale).abs() < 1.0);
        assert!((h - 1000.0 * expected_scale).abs() < 1.0);
        // Image should be shorter than display (y > 0).
        assert!(y > 0.0);
    }

    #[test]
    fn image_rect_zero_dimensions() {
        let (x, y, w, h) = compute_image_rect(0.0, 0.0, 100.0, 100.0, ImageFit::Fill);
        assert!((x).abs() < f32::EPSILON);
        assert!((y).abs() < f32::EPSILON);
        assert!((w).abs() < f32::EPSILON);
        assert!((h).abs() < f32::EPSILON);
    }

    // ------------------------------------------------------------------
    // parse_hex_color
    // ------------------------------------------------------------------

    #[test]
    fn parse_hex_valid() {
        let c = parse_hex_color("1E1E2E").expect("should parse");
        assert_eq!(c, Color::from_hex(0x1E1E2E));
    }

    #[test]
    fn parse_hex_with_hash() {
        let c = parse_hex_color("#89B4FA").expect("should parse");
        assert_eq!(c, Color::from_hex(0x89B4FA));
    }

    #[test]
    fn parse_hex_invalid_length() {
        assert!(parse_hex_color("FFF").is_err());
    }

    #[test]
    fn parse_hex_invalid_chars() {
        assert!(parse_hex_color("ZZZZZZ").is_err());
    }

    // ------------------------------------------------------------------
    // apply_config
    // ------------------------------------------------------------------

    #[test]
    fn apply_config_resets_state() {
        let mut mgr = WallpaperManager::new();
        mgr.set_image("/test.png", ImageFit::Fill);
        let old_id = mgr.current_image_id();

        let config = WallpaperConfig {
            mode: WallpaperMode::SingleImage,
            image_path: "/other.png".into(),
            ..WallpaperConfig::default()
        };
        mgr.apply_config(config);

        // Should have a new image ID (not the old one or zero).
        assert_ne!(mgr.current_image_id(), 0);
        assert_ne!(mgr.current_image_id(), old_id);
        assert_eq!(mgr.config.image_path, "/other.png");
    }

    #[test]
    fn apply_config_slideshow_creates_state() {
        let mut mgr = WallpaperManager::new();
        let config = WallpaperConfig {
            mode: WallpaperMode::Slideshow,
            slideshow_dir: "/pics".into(),
            ..WallpaperConfig::default()
        };
        mgr.apply_config(config);
        assert!(mgr.slideshow.is_some());
    }

    // ------------------------------------------------------------------
    // Manager accessors
    // ------------------------------------------------------------------

    #[test]
    fn current_image_path_solid_returns_none() {
        let mgr = WallpaperManager::new();
        assert!(mgr.current_image_path().is_none());
    }

    #[test]
    fn current_image_path_single_image() {
        let mut mgr = WallpaperManager::new();
        mgr.set_image("/wallpaper.png", ImageFit::Fill);
        assert_eq!(mgr.current_image_path(), Some("/wallpaper.png"));
    }

    #[test]
    fn alloc_image_id_never_zero() {
        let mut mgr = WallpaperManager::new();
        for _ in 0..100 {
            let id = mgr.alloc_image_id();
            assert_ne!(id, 0);
        }
    }

    #[test]
    fn slideshow_interval_minimum_is_one() {
        let mut mgr = WallpaperManager::new();
        mgr.set_slideshow("/wp", 0, false);
        assert_eq!(mgr.config.slideshow_interval_secs, 1);
    }

    // ------------------------------------------------------------------
    // ConfigError display
    // ------------------------------------------------------------------

    #[test]
    fn config_error_display() {
        let e = ConfigError::MissingKey("mode".into());
        assert_eq!(format!("{e}"), "missing key: mode");

        let e = ConfigError::InvalidValue("color: xyz".into());
        assert_eq!(format!("{e}"), "invalid value: color: xyz");

        let e = ConfigError::UnknownMode("plasma".into());
        assert_eq!(format!("{e}"), "unknown wallpaper mode: plasma");

        let e = ConfigError::UnknownFit("warp".into());
        assert_eq!(format!("{e}"), "unknown image fit: warp");
    }
}
