//! Desktop wallpaper — background image management and display settings.
//!
//! Manages the desktop background image, including static images,
//! slideshows, animated/dynamic wallpapers, and fit/fill options.
//!
//! ## Design Reference
//!
//! design.txt line 1245: "desktop background image - can set animated
//! background? - not just a video, but a program constantly changing it"
//!
//! design.txt line 1246: "Let the user opt to choose a random desktop
//! background each boot-up, or a different one every day"
//!
//! design.txt line 1247: "login screen background image - easy way to
//! make the two the same"
//!
//! ## Architecture
//!
//! ```text
//! Compositor
//!   → wallpaper::current()
//!   → render background based on FitMode and offset
//!
//! Settings panel / kshell
//!   → wallpaper::set_image(path)
//!   → wallpaper::set_fit_mode(mode)
//!   → wallpaper::set_slideshow(paths, interval)
//! ```
//!
//! ## Fit Modes
//!
//! When the image doesn't match desktop dimensions:
//! - **Fill**: Scale to fill, crop excess (user can pan the crop)
//! - **Fit**: Scale to fit inside, letterbox/pillarbox with color
//! - **Stretch**: Scale to exact desktop size (distorts)
//! - **Center**: No scaling, center on desktop, fill rest with color
//! - **Tile**: Repeat image across desktop
//! - **Span**: Stretch across all monitors

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of images in a slideshow.
const MAX_SLIDESHOW: usize = 256;

/// Maximum wallpaper history entries.
const MAX_HISTORY: usize = 64;

/// Maximum exclusion patterns.
const MAX_EXCLUSIONS: usize = 64;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// How the wallpaper image is fitted to the desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    /// Scale to fill, crop excess — user can adjust offset.
    Fill,
    /// Scale to fit inside, letterbox/pillarbox with background color.
    Fit,
    /// Stretch to exact desktop size (may distort).
    Stretch,
    /// No scaling, center image, fill rest with color.
    Center,
    /// Repeat image across the desktop.
    Tile,
    /// Span across all monitors.
    Span,
}

impl FitMode {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Fill => "Fill",
            Self::Fit => "Fit",
            Self::Stretch => "Stretch",
            Self::Center => "Center",
            Self::Tile => "Tile",
            Self::Span => "Span",
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "fill" => Some(Self::Fill),
            "fit" => Some(Self::Fit),
            "stretch" => Some(Self::Stretch),
            "center" => Some(Self::Center),
            "tile" => Some(Self::Tile),
            "span" => Some(Self::Span),
            _ => None,
        }
    }
}

/// Wallpaper type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallpaperKind {
    /// Static image file.
    Static,
    /// Slideshow of images.
    Slideshow,
    /// Animated wallpaper (video or program).
    Animated,
    /// Dynamic wallpaper (changes based on time of day).
    Dynamic,
    /// Solid color only (no image).
    SolidColor,
}

impl WallpaperKind {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Static => "Static",
            Self::Slideshow => "Slideshow",
            Self::Animated => "Animated",
            Self::Dynamic => "Dynamic",
            Self::SolidColor => "Solid Color",
        }
    }
}

/// Slideshow shuffle mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShuffleMode {
    /// Play images in order.
    Sequential,
    /// Randomize order.
    Random,
}

impl ShuffleMode {
    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "seq" | "sequential" | "ordered" => Some(Self::Sequential),
            "random" | "shuffle" => Some(Self::Random),
            _ => None,
        }
    }
}

/// Current wallpaper configuration.
#[derive(Debug, Clone)]
pub struct WallpaperConfig {
    /// Kind of wallpaper.
    pub kind: WallpaperKind,
    /// Path to current image file (empty for SolidColor).
    pub image_path: String,
    /// Fit mode.
    pub fit_mode: FitMode,
    /// Background/letterbox color (hex, e.g., "#000000").
    pub background_color: String,
    /// Offset for Fill mode: panning the crop (0.0-1.0 for x, y).
    pub offset_x: f32,
    pub offset_y: f32,
    /// Slideshow images.
    pub slideshow_paths: Vec<String>,
    /// Slideshow interval in seconds.
    pub slideshow_interval_secs: u64,
    /// Current slideshow index.
    pub slideshow_index: usize,
    /// Shuffle mode for slideshow.
    pub shuffle: ShuffleMode,
    /// Whether slideshow auto-advances.
    pub slideshow_running: bool,
    /// Path to animated wallpaper program/video.
    pub animated_source: String,
    /// Whether to use same wallpaper for login screen.
    pub use_for_login: bool,
    /// Per-monitor wallpaper override (monitor_id → path).
    pub per_monitor: Vec<(String, String)>,
    /// Change wallpaper randomly on boot.
    pub random_on_boot: bool,
    /// Change wallpaper daily.
    pub change_daily: bool,
    /// Excluded patterns (file paths that won't be picked randomly).
    pub exclusions: Vec<String>,
}

impl WallpaperConfig {
    fn new() -> Self {
        Self {
            kind: WallpaperKind::SolidColor,
            image_path: String::new(),
            fit_mode: FitMode::Fill,
            background_color: String::from("#1a1a2e"),
            offset_x: 0.5,
            offset_y: 0.5,
            slideshow_paths: Vec::new(),
            slideshow_interval_secs: 300,
            slideshow_index: 0,
            shuffle: ShuffleMode::Sequential,
            slideshow_running: false,
            animated_source: String::new(),
            use_for_login: true,
            per_monitor: Vec::new(),
            random_on_boot: false,
            change_daily: false,
            exclusions: Vec::new(),
        }
    }
}

/// A wallpaper history entry.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// Image path.
    pub path: String,
    /// When it was set (nanoseconds).
    pub set_at_ns: u64,
    /// How it was set (manual/slideshow/random/boot).
    pub source: String,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct WallpaperState {
    config: WallpaperConfig,
    history: Vec<HistoryEntry>,
}

impl WallpaperState {
    const fn new() -> Self {
        Self {
            config: WallpaperConfig {
                kind: WallpaperKind::SolidColor,
                image_path: String::new(),
                fit_mode: FitMode::Fill,
                background_color: String::new(),
                offset_x: 0.5,
                offset_y: 0.5,
                slideshow_paths: Vec::new(),
                slideshow_interval_secs: 300,
                slideshow_index: 0,
                shuffle: ShuffleMode::Sequential,
                slideshow_running: false,
                animated_source: String::new(),
                use_for_login: true,
                per_monitor: Vec::new(),
                random_on_boot: false,
                change_daily: false,
                exclusions: Vec::new(),
            },
            history: Vec::new(),
        }
    }

    fn add_history(&mut self, path: &str, source: &str) {
        let now = crate::timekeeping::clock_monotonic();
        if self.history.len() >= MAX_HISTORY {
            self.history.remove(0);
        }
        self.history.push(HistoryEntry {
            path: String::from(path),
            set_at_ns: now,
            source: String::from(source),
        });
    }
}

static WALLPAPER: Mutex<WallpaperState> = Mutex::new(WallpaperState::new());
static SET_COUNT: AtomicU64 = AtomicU64::new(0);
static ADVANCE_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Get current wallpaper configuration.
pub fn current() -> WallpaperConfig {
    WALLPAPER.lock().config.clone()
}

/// Set a static image as the wallpaper.
pub fn set_image(path: &str) -> KernelResult<()> {
    if path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    SET_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut state = WALLPAPER.lock();
    state.config.kind = WallpaperKind::Static;
    state.config.image_path = String::from(path);
    state.config.slideshow_running = false;
    state.add_history(path, "manual");
    Ok(())
}

/// Set wallpaper to a solid color (no image).
pub fn set_solid_color(color: &str) -> KernelResult<()> {
    if color.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    SET_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut state = WALLPAPER.lock();
    state.config.kind = WallpaperKind::SolidColor;
    state.config.image_path = String::new();
    state.config.background_color = String::from(color);
    state.config.slideshow_running = false;
    Ok(())
}

/// Set the fit mode.
pub fn set_fit_mode(mode: FitMode) {
    WALLPAPER.lock().config.fit_mode = mode;
}

/// Set the background/letterbox color.
pub fn set_background_color(color: &str) {
    WALLPAPER.lock().config.background_color = String::from(color);
}

/// Set the crop offset for Fill mode (both 0.0-1.0).
pub fn set_offset(x: f32, y: f32) {
    let mut state = WALLPAPER.lock();
    state.config.offset_x = x.clamp(0.0, 1.0);
    state.config.offset_y = y.clamp(0.0, 1.0);
}

// ---------------------------------------------------------------------------
// Slideshow
// ---------------------------------------------------------------------------

/// Set up a slideshow.
pub fn set_slideshow(paths: &[&str], interval_secs: u64) -> KernelResult<()> {
    if paths.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    if paths.len() > MAX_SLIDESHOW {
        return Err(KernelError::ResourceExhausted);
    }
    SET_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut state = WALLPAPER.lock();
    state.config.kind = WallpaperKind::Slideshow;
    state.config.slideshow_paths = paths.iter().map(|p| String::from(*p)).collect();
    state.config.slideshow_interval_secs = if interval_secs == 0 { 300 } else { interval_secs };
    state.config.slideshow_index = 0;
    state.config.slideshow_running = true;
    if let Some(first) = paths.first() {
        state.config.image_path = String::from(*first);
        state.add_history(first, "slideshow");
    }
    Ok(())
}

/// Add an image to the slideshow.
pub fn slideshow_add(path: &str) -> KernelResult<()> {
    if path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = WALLPAPER.lock();
    if state.config.slideshow_paths.len() >= MAX_SLIDESHOW {
        return Err(KernelError::ResourceExhausted);
    }
    state.config.slideshow_paths.push(String::from(path));
    Ok(())
}

/// Remove an image from the slideshow by index.
pub fn slideshow_remove(index: usize) -> KernelResult<()> {
    let mut state = WALLPAPER.lock();
    if index >= state.config.slideshow_paths.len() {
        return Err(KernelError::NotFound);
    }
    state.config.slideshow_paths.remove(index);
    // Adjust current index if needed.
    if state.config.slideshow_index >= state.config.slideshow_paths.len()
        && !state.config.slideshow_paths.is_empty()
    {
        state.config.slideshow_index = 0;
    }
    Ok(())
}

/// Advance to next slideshow image.
pub fn slideshow_next() -> KernelResult<String> {
    ADVANCE_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut state = WALLPAPER.lock();
    if state.config.slideshow_paths.is_empty() {
        return Err(KernelError::NotFound);
    }
    let next_idx = (state.config.slideshow_index.wrapping_add(1))
        % state.config.slideshow_paths.len();
    state.config.slideshow_index = next_idx;
    let path = state.config.slideshow_paths.get(next_idx)
        .cloned()
        .unwrap_or_default();
    state.config.image_path = path.clone();
    state.add_history(&path, "slideshow");
    Ok(path)
}

/// Go to previous slideshow image.
pub fn slideshow_prev() -> KernelResult<String> {
    let mut state = WALLPAPER.lock();
    if state.config.slideshow_paths.is_empty() {
        return Err(KernelError::NotFound);
    }
    let len = state.config.slideshow_paths.len();
    let prev_idx = if state.config.slideshow_index == 0 {
        len.saturating_sub(1)
    } else {
        state.config.slideshow_index.saturating_sub(1)
    };
    state.config.slideshow_index = prev_idx;
    let path = state.config.slideshow_paths.get(prev_idx)
        .cloned()
        .unwrap_or_default();
    state.config.image_path = path.clone();
    state.add_history(&path, "slideshow");
    Ok(path)
}

/// Set slideshow interval.
pub fn set_slideshow_interval(secs: u64) {
    let mut state = WALLPAPER.lock();
    state.config.slideshow_interval_secs = if secs == 0 { 300 } else { secs };
}

/// Pause/resume slideshow.
pub fn set_slideshow_running(running: bool) {
    WALLPAPER.lock().config.slideshow_running = running;
}

/// Set shuffle mode.
pub fn set_shuffle(mode: ShuffleMode) {
    WALLPAPER.lock().config.shuffle = mode;
}

// ---------------------------------------------------------------------------
// Animated / Dynamic
// ---------------------------------------------------------------------------

/// Set an animated wallpaper source (video path or program path).
pub fn set_animated(source: &str) -> KernelResult<()> {
    if source.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    SET_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut state = WALLPAPER.lock();
    state.config.kind = WallpaperKind::Animated;
    state.config.animated_source = String::from(source);
    state.config.image_path = String::from(source);
    state.config.slideshow_running = false;
    state.add_history(source, "animated");
    Ok(())
}

/// Set a dynamic wallpaper (changes with time of day).
pub fn set_dynamic(source: &str) -> KernelResult<()> {
    if source.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    SET_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut state = WALLPAPER.lock();
    state.config.kind = WallpaperKind::Dynamic;
    state.config.animated_source = String::from(source);
    state.config.image_path = String::from(source);
    state.config.slideshow_running = false;
    state.add_history(source, "dynamic");
    Ok(())
}

// ---------------------------------------------------------------------------
// Login screen
// ---------------------------------------------------------------------------

/// Set whether the desktop wallpaper is also used for the login screen.
pub fn set_use_for_login(use_it: bool) {
    WALLPAPER.lock().config.use_for_login = use_it;
}

/// Get whether the login screen uses the desktop wallpaper.
pub fn use_for_login() -> bool {
    WALLPAPER.lock().config.use_for_login
}

// ---------------------------------------------------------------------------
// Per-monitor
// ---------------------------------------------------------------------------

/// Set a wallpaper for a specific monitor.
pub fn set_per_monitor(monitor_id: &str, path: &str) -> KernelResult<()> {
    if monitor_id.is_empty() || path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = WALLPAPER.lock();
    // Update existing or add new.
    for entry in &mut state.config.per_monitor {
        if entry.0 == monitor_id {
            entry.1 = String::from(path);
            return Ok(());
        }
    }
    state.config.per_monitor.push((String::from(monitor_id), String::from(path)));
    Ok(())
}

/// Clear per-monitor override (use global wallpaper).
pub fn clear_per_monitor(monitor_id: &str) -> KernelResult<()> {
    let mut state = WALLPAPER.lock();
    let before = state.config.per_monitor.len();
    state.config.per_monitor.retain(|e| e.0 != monitor_id);
    if state.config.per_monitor.len() == before {
        return Err(KernelError::NotFound);
    }
    Ok(())
}

/// Get wallpaper path for a specific monitor (falls back to global).
pub fn wallpaper_for_monitor(monitor_id: &str) -> String {
    let state = WALLPAPER.lock();
    for entry in &state.config.per_monitor {
        if entry.0 == monitor_id {
            return entry.1.clone();
        }
    }
    state.config.image_path.clone()
}

// ---------------------------------------------------------------------------
// Random / daily options
// ---------------------------------------------------------------------------

/// Set whether to change wallpaper randomly on boot.
pub fn set_random_on_boot(enabled: bool) {
    WALLPAPER.lock().config.random_on_boot = enabled;
}

/// Set whether to change wallpaper daily.
pub fn set_change_daily(enabled: bool) {
    WALLPAPER.lock().config.change_daily = enabled;
}

/// Add a path pattern to the exclusion list for random selection.
pub fn add_exclusion(pattern: &str) -> KernelResult<()> {
    if pattern.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = WALLPAPER.lock();
    if state.config.exclusions.len() >= MAX_EXCLUSIONS {
        return Err(KernelError::ResourceExhausted);
    }
    state.config.exclusions.push(String::from(pattern));
    Ok(())
}

/// Remove an exclusion.
pub fn remove_exclusion(index: usize) -> KernelResult<()> {
    let mut state = WALLPAPER.lock();
    if index >= state.config.exclusions.len() {
        return Err(KernelError::NotFound);
    }
    state.config.exclusions.remove(index);
    Ok(())
}

/// Check if a path matches any exclusion pattern (simple substring match).
pub fn is_excluded(path: &str) -> bool {
    let state = WALLPAPER.lock();
    let path_lower = path.to_ascii_lowercase();
    state.config.exclusions.iter().any(|exc| {
        let exc_lower = exc.to_ascii_lowercase();
        path_lower.contains(&exc_lower)
    })
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

/// Get wallpaper history (most recent first).
pub fn history() -> Vec<HistoryEntry> {
    let state = WALLPAPER.lock();
    let mut h = state.history.clone();
    h.reverse();
    h
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (slideshow_count, history_count, set_count, advance_count).
pub fn stats() -> (usize, usize, u64, u64) {
    let state = WALLPAPER.lock();
    (
        state.config.slideshow_paths.len(),
        state.history.len(),
        SET_COUNT.load(Ordering::Relaxed),
        ADVANCE_COUNT.load(Ordering::Relaxed),
    )
}

/// Reset counters.
pub fn reset_stats() {
    SET_COUNT.store(0, Ordering::Relaxed);
    ADVANCE_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all data.
pub fn clear_all() {
    let mut state = WALLPAPER.lock();
    state.config = WallpaperConfig::new();
    state.history.clear();
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the wallpaper system.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    reset_stats();

    // Test 1: Set static image.
    serial_println!("  wallpaper::test 1: static image");
    set_image("/usr/share/wallpapers/mountain.jpg")?;
    let cfg = current();
    assert_eq!(cfg.kind, WallpaperKind::Static);
    assert_eq!(cfg.image_path, "/usr/share/wallpapers/mountain.jpg");
    assert!(!cfg.image_path.is_empty());

    // Test 2: Fit mode and color.
    serial_println!("  wallpaper::test 2: fit mode and color");
    set_fit_mode(FitMode::Center);
    set_background_color("#2d2d44");
    let cfg2 = current();
    assert_eq!(cfg2.fit_mode, FitMode::Center);
    assert_eq!(cfg2.background_color, "#2d2d44");
    set_offset(0.3, 0.7);
    let cfg3 = current();
    assert!((cfg3.offset_x - 0.3).abs() < 0.01);
    assert!((cfg3.offset_y - 0.7).abs() < 0.01);

    // Test 3: Slideshow.
    serial_println!("  wallpaper::test 3: slideshow");
    let images = ["/wp/a.jpg", "/wp/b.jpg", "/wp/c.jpg"];
    set_slideshow(&images, 60)?;
    let cfg4 = current();
    assert_eq!(cfg4.kind, WallpaperKind::Slideshow);
    assert_eq!(cfg4.slideshow_paths.len(), 3);
    assert!(cfg4.slideshow_running);
    let next = slideshow_next()?;
    assert_eq!(next, "/wp/b.jpg");
    let prev = slideshow_prev()?;
    assert_eq!(prev, "/wp/a.jpg");
    slideshow_add("/wp/d.jpg")?;
    assert_eq!(current().slideshow_paths.len(), 4);
    slideshow_remove(3)?;
    assert_eq!(current().slideshow_paths.len(), 3);

    // Test 4: Solid color.
    serial_println!("  wallpaper::test 4: solid color");
    set_solid_color("#000000")?;
    let cfg5 = current();
    assert_eq!(cfg5.kind, WallpaperKind::SolidColor);
    assert!(cfg5.image_path.is_empty());

    // Test 5: Per-monitor.
    serial_println!("  wallpaper::test 5: per-monitor");
    set_image("/wp/main.jpg")?;
    set_per_monitor("HDMI-1", "/wp/second.jpg")?;
    assert_eq!(wallpaper_for_monitor("HDMI-1"), "/wp/second.jpg");
    assert_eq!(wallpaper_for_monitor("DP-1"), "/wp/main.jpg");
    clear_per_monitor("HDMI-1")?;
    assert_eq!(wallpaper_for_monitor("HDMI-1"), "/wp/main.jpg");

    // Test 6: Login screen and random options.
    serial_println!("  wallpaper::test 6: login and random options");
    set_use_for_login(false);
    assert!(!use_for_login());
    set_use_for_login(true);
    assert!(use_for_login());
    set_random_on_boot(true);
    set_change_daily(true);
    let cfg6 = current();
    assert!(cfg6.random_on_boot);
    assert!(cfg6.change_daily);

    // Test 7: Exclusions and history.
    serial_println!("  wallpaper::test 7: exclusions and history");
    add_exclusion("animated")?;
    assert!(is_excluded("/wp/animated_sunset.mp4"));
    assert!(!is_excluded("/wp/static_mountain.jpg"));
    remove_exclusion(0)?;
    assert!(!is_excluded("/wp/animated_sunset.mp4"));
    let hist = history();
    assert!(hist.len() >= 2); // We set several images above.

    // Cleanup.
    clear_all();
    reset_stats();

    serial_println!("  wallpaper: all tests passed");
    Ok(())
}
