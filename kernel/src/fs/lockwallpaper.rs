//! Lock Screen Wallpaper — lock screen background management.
//!
//! Manages lock screen backgrounds separately from desktop wallpapers,
//! with slideshow, spotlight, and custom image support.
//!
//! ## Architecture
//!
//! ```text
//! Screen locks
//!   → lockwallpaper::get_current() → background image
//!   → lockwallpaper::rotate() → next in slideshow
//!
//! Integration:
//!   → screenlock (lock screen)
//!   → wallpaper (desktop wallpaper)
//!   → loginscreen (login screen)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Lock wallpaper mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockWallpaperMode {
    /// Static image.
    Static,
    /// Slideshow from folder.
    Slideshow,
    /// System spotlight (curated daily images).
    Spotlight,
    /// Solid color.
    SolidColor,
    /// Same as desktop wallpaper.
    SameAsDesktop,
}

impl LockWallpaperMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Static => "Static",
            Self::Slideshow => "Slideshow",
            Self::Spotlight => "Spotlight",
            Self::SolidColor => "Solid Color",
            Self::SameAsDesktop => "Same as Desktop",
        }
    }
}

/// Fit mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    Fill,
    Fit,
    Stretch,
    Tile,
    Center,
    Span,
}

impl FitMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fill => "Fill",
            Self::Fit => "Fit",
            Self::Stretch => "Stretch",
            Self::Tile => "Tile",
            Self::Center => "Center",
            Self::Span => "Span",
        }
    }
}

/// Lock wallpaper configuration.
#[derive(Debug, Clone)]
pub struct LockWallpaperConfig {
    pub mode: LockWallpaperMode,
    pub current_image: String,
    pub slideshow_dir: String,
    pub slideshow_interval_secs: u32,
    /// Images in slideshow rotation.
    pub slideshow_images: Vec<String>,
    pub slideshow_index: usize,
    pub fit_mode: FitMode,
    /// Solid color as RGB hex string.
    pub solid_color: String,
    pub show_clock: bool,
    pub show_notifications: bool,
    pub blur_behind: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    config: LockWallpaperConfig,
    total_rotations: u64,
    total_changes: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        config: LockWallpaperConfig {
            mode: LockWallpaperMode::Spotlight,
            current_image: String::from("/sys/wallpapers/lock_default.png"),
            slideshow_dir: String::new(),
            slideshow_interval_secs: 60,
            slideshow_images: Vec::new(),
            slideshow_index: 0,
            fit_mode: FitMode::Fill,
            solid_color: String::from("#1a1a2e"),
            show_clock: true,
            show_notifications: true,
            blur_behind: false,
        },
        total_rotations: 0,
        total_changes: 0,
        ops: 0,
    });
}

/// Set mode.
pub fn set_mode(mode: LockWallpaperMode) -> KernelResult<()> {
    with_state(|state| {
        state.config.mode = mode;
        state.total_changes += 1;
        Ok(())
    })
}

/// Set static image.
pub fn set_image(path: &str) -> KernelResult<()> {
    with_state(|state| {
        state.config.current_image = String::from(path);
        state.config.mode = LockWallpaperMode::Static;
        state.total_changes += 1;
        Ok(())
    })
}

/// Set slideshow directory.
pub fn set_slideshow_dir(dir: &str, interval_secs: u32) -> KernelResult<()> {
    with_state(|state| {
        state.config.slideshow_dir = String::from(dir);
        state.config.slideshow_interval_secs = interval_secs.clamp(10, 86400);
        state.config.mode = LockWallpaperMode::Slideshow;
        state.total_changes += 1;
        Ok(())
    })
}

/// Add image to slideshow.
pub fn add_slideshow_image(path: &str) -> KernelResult<()> {
    with_state(|state| {
        state.config.slideshow_images.push(String::from(path));
        Ok(())
    })
}

/// Rotate to next slideshow image.
pub fn rotate() -> KernelResult<String> {
    with_state(|state| {
        if state.config.slideshow_images.is_empty() {
            return Ok(state.config.current_image.clone());
        }
        state.config.slideshow_index = (state.config.slideshow_index + 1) % state.config.slideshow_images.len();
        let img = state.config.slideshow_images[state.config.slideshow_index].clone();
        state.config.current_image = img.clone();
        state.total_rotations += 1;
        Ok(img)
    })
}

/// Set fit mode.
pub fn set_fit_mode(fit: FitMode) -> KernelResult<()> {
    with_state(|state| {
        state.config.fit_mode = fit;
        state.total_changes += 1;
        Ok(())
    })
}

/// Set solid color.
pub fn set_solid_color(color: &str) -> KernelResult<()> {
    with_state(|state| {
        state.config.solid_color = String::from(color);
        state.config.mode = LockWallpaperMode::SolidColor;
        state.total_changes += 1;
        Ok(())
    })
}

/// Set clock/notification visibility.
pub fn set_overlays(show_clock: bool, show_notifications: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.show_clock = show_clock;
        state.config.show_notifications = show_notifications;
        state.total_changes += 1;
        Ok(())
    })
}

/// Set blur behind.
pub fn set_blur(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.blur_behind = enabled;
        state.total_changes += 1;
        Ok(())
    })
}

/// Get current config.
pub fn get_config() -> Option<LockWallpaperConfig> {
    STATE.lock().as_ref().map(|s| s.config.clone())
}

/// Get current image path.
pub fn current_image() -> String {
    STATE.lock().as_ref().map_or(String::new(), |s| s.config.current_image.clone())
}

/// Statistics: (total_rotations, total_changes, ops).
pub fn stats() -> (u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.total_rotations, s.total_changes, s.ops),
        None => (0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("lockwallpaper::self_test() — running tests...");
    init_defaults();

    // 1: Default spotlight mode.
    let cfg = get_config().expect("config");
    assert_eq!(cfg.mode, LockWallpaperMode::Spotlight);
    assert!(cfg.show_clock);
    crate::serial_println!("  [1/8] default config: OK");

    // 2: Set static image.
    set_image("/pictures/lock.jpg").expect("img");
    let cfg = get_config().expect("config2");
    assert_eq!(cfg.mode, LockWallpaperMode::Static);
    assert_eq!(cfg.current_image, "/pictures/lock.jpg");
    crate::serial_println!("  [2/8] static image: OK");

    // 3: Slideshow setup.
    set_slideshow_dir("/pictures/slideshow", 30).expect("dir");
    add_slideshow_image("/pictures/a.jpg").expect("add1");
    add_slideshow_image("/pictures/b.jpg").expect("add2");
    add_slideshow_image("/pictures/c.jpg").expect("add3");
    crate::serial_println!("  [3/8] slideshow setup: OK");

    // 4: Rotate.
    let img = rotate().expect("rotate");
    assert_eq!(img, "/pictures/b.jpg"); // index 0→1
    let img = rotate().expect("rotate2");
    assert_eq!(img, "/pictures/c.jpg"); // index 1→2
    crate::serial_println!("  [4/8] rotate: OK");

    // 5: Fit mode.
    set_fit_mode(FitMode::Center).expect("fit");
    let cfg = get_config().expect("config3");
    assert_eq!(cfg.fit_mode, FitMode::Center);
    crate::serial_println!("  [5/8] fit mode: OK");

    // 6: Solid color.
    set_solid_color("#2d3436").expect("color");
    let cfg = get_config().expect("config4");
    assert_eq!(cfg.mode, LockWallpaperMode::SolidColor);
    crate::serial_println!("  [6/8] solid color: OK");

    // 7: Overlays.
    set_overlays(false, false).expect("overlay");
    let cfg = get_config().expect("config5");
    assert!(!cfg.show_clock);
    assert!(!cfg.show_notifications);
    crate::serial_println!("  [7/8] overlays: OK");

    // 8: Stats.
    let (rotations, changes, ops) = stats();
    assert_eq!(rotations, 2);
    assert!(changes >= 5);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("lockwallpaper::self_test() — all 8 tests passed");
}
