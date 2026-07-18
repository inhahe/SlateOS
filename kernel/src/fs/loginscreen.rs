//! Login screen configuration — background, layout, and behaviour settings.
//!
//! Manages the login/lock screen appearance and behaviour including background
//! image synchronisation with the desktop wallpaper.
//!
//! ## Design Reference
//!
//! design.txt line 1247: login screen background image - easy way to make
//!   the two the same (matching desktop background)
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Login Screen
//!   → loginscreen::config() → current settings
//!   → loginscreen::sync_with_desktop() → copy desktop wallpaper
//!
//! Login manager
//!   → loginscreen::config() → read background, layout, etc.
//! ```

#![allow(dead_code)]

use alloc::string::String;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::KernelResult;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Background mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundMode {
    /// Static image.
    Image,
    /// Solid colour.
    SolidColor,
    /// Gradient (two colours).
    Gradient,
    /// Slideshow (rotate images).
    Slideshow,
    /// Blur of desktop wallpaper.
    BlurDesktop,
}

/// Image fit mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    /// Fill the screen (may crop).
    Fill,
    /// Fit within screen (may letterbox).
    Fit,
    /// Stretch to fill (distorts).
    Stretch,
    /// Centre at original size.
    Center,
    /// Tile the image.
    Tile,
}

/// Clock position on the login screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockPosition {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
    Hidden,
}

/// User list display mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserListMode {
    /// Show all users with avatars.
    ShowAll,
    /// Show only recent users.
    RecentOnly,
    /// Username text entry (no list).
    TextEntry,
    /// Hide user list entirely.
    Hidden,
}

/// Complete login screen configuration.
#[derive(Debug, Clone)]
pub struct LoginConfig {
    /// Background mode.
    pub background_mode: BackgroundMode,
    /// Background image path.
    pub background_path: String,
    /// Background colour (hex RRGGBB).
    pub background_color: String,
    /// Gradient end colour (hex RRGGBB).
    pub gradient_end: String,
    /// Image fit mode.
    pub fit_mode: FitMode,
    /// Blur amount (0-100, for BlurDesktop mode).
    pub blur_amount: u8,
    /// Slideshow interval in seconds.
    pub slideshow_interval_s: u32,
    /// Slideshow image directory.
    pub slideshow_dir: String,
    /// Whether background matches desktop wallpaper.
    pub synced_with_desktop: bool,
    /// Clock position.
    pub clock_position: ClockPosition,
    /// Whether to show date.
    pub show_date: bool,
    /// Whether to show weather (if network available).
    pub show_weather: bool,
    /// User list mode.
    pub user_list: UserListMode,
    /// Whether to show last login time.
    pub show_last_login: bool,
    /// Whether to enable on-screen keyboard.
    pub virtual_keyboard: bool,
    /// Whether to show accessibility options button.
    pub show_a11y: bool,
    /// Whether to show power options (shutdown/reboot).
    pub show_power: bool,
    /// Screen lock timeout in seconds (0 = never).
    pub lock_timeout_s: u32,
    /// Custom message / MOTD.
    pub message: String,
    /// Logo / branding image path.
    pub logo_path: String,
}

impl Default for LoginConfig {
    fn default() -> Self {
        Self {
            background_mode: BackgroundMode::BlurDesktop,
            background_path: String::new(),
            background_color: String::from("1a1a2e"),
            gradient_end: String::from("16213e"),
            fit_mode: FitMode::Fill,
            blur_amount: 30,
            slideshow_interval_s: 30,
            slideshow_dir: String::new(),
            synced_with_desktop: true,
            clock_position: ClockPosition::TopCenter,
            show_date: true,
            show_weather: false,
            user_list: UserListMode::ShowAll,
            show_last_login: true,
            virtual_keyboard: false,
            show_a11y: true,
            show_power: true,
            lock_timeout_s: 300,
            message: String::new(),
            logo_path: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    config: LoginConfig,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    config: LoginConfig {
        background_mode: BackgroundMode::BlurDesktop,
        background_path: String::new(),
        background_color: String::new(),
        gradient_end: String::new(),
        fit_mode: FitMode::Fill,
        blur_amount: 30,
        slideshow_interval_s: 30,
        slideshow_dir: String::new(),
        synced_with_desktop: true,
        clock_position: ClockPosition::TopCenter,
        show_date: true,
        show_weather: false,
        user_list: UserListMode::ShowAll,
        show_last_login: true,
        virtual_keyboard: false,
        show_a11y: true,
        show_power: true,
        lock_timeout_s: 300,
        message: String::new(),
        logo_path: String::new(),
    },
    changes: 0,
});

static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Get current login screen configuration.
pub fn config() -> LoginConfig {
    STATE.lock().config.clone()
}

/// Set background mode.
pub fn set_background_mode(mode: BackgroundMode) {
    let mut state = STATE.lock();
    state.config.background_mode = mode;
    if mode == BackgroundMode::BlurDesktop {
        state.config.synced_with_desktop = true;
    }
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Set background image path.
pub fn set_background_image(path: &str) {
    let mut state = STATE.lock();
    state.config.background_path = String::from(path);
    state.config.background_mode = BackgroundMode::Image;
    state.config.synced_with_desktop = false;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Set background colour.
pub fn set_background_color(hex: &str) {
    let mut state = STATE.lock();
    state.config.background_color = String::from(hex);
    state.changes += 1;
}

/// Set gradient end colour.
pub fn set_gradient_end(hex: &str) {
    let mut state = STATE.lock();
    state.config.gradient_end = String::from(hex);
    state.changes += 1;
}

/// Set fit mode.
pub fn set_fit_mode(mode: FitMode) {
    let mut state = STATE.lock();
    state.config.fit_mode = mode;
    state.changes += 1;
}

/// Set blur amount (0-100).
pub fn set_blur(amount: u8) {
    let mut state = STATE.lock();
    state.config.blur_amount = amount.min(100);
    state.changes += 1;
}

/// Sync login screen with desktop wallpaper.
pub fn sync_with_desktop() {
    let mut state = STATE.lock();
    state.config.synced_with_desktop = true;
    state.config.background_mode = BackgroundMode::BlurDesktop;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Set clock position.
pub fn set_clock_position(pos: ClockPosition) {
    let mut state = STATE.lock();
    state.config.clock_position = pos;
    state.changes += 1;
}

/// Set whether to show date.
pub fn set_show_date(show: bool) {
    let mut state = STATE.lock();
    state.config.show_date = show;
    state.changes += 1;
}

/// Set user list mode.
pub fn set_user_list(mode: UserListMode) {
    let mut state = STATE.lock();
    state.config.user_list = mode;
    state.changes += 1;
}

/// Set virtual keyboard visibility.
pub fn set_virtual_keyboard(show: bool) {
    let mut state = STATE.lock();
    state.config.virtual_keyboard = show;
    state.changes += 1;
}

/// Set power options visibility.
pub fn set_show_power(show: bool) {
    let mut state = STATE.lock();
    state.config.show_power = show;
    state.changes += 1;
}

/// Set lock timeout.
pub fn set_lock_timeout(seconds: u32) {
    let mut state = STATE.lock();
    state.config.lock_timeout_s = seconds;
    state.changes += 1;
}

/// Set custom message.
pub fn set_message(msg: &str) {
    let mut state = STATE.lock();
    state.config.message = String::from(msg);
    state.changes += 1;
}

/// Set logo path.
pub fn set_logo(path: &str) {
    let mut state = STATE.lock();
    state.config.logo_path = String::from(path);
    state.changes += 1;
}

/// Set slideshow directory.
pub fn set_slideshow_dir(dir: &str) {
    let mut state = STATE.lock();
    state.config.slideshow_dir = String::from(dir);
    state.changes += 1;
}

/// Set slideshow interval.
pub fn set_slideshow_interval(seconds: u32) {
    let mut state = STATE.lock();
    state.config.slideshow_interval_s = seconds.clamp(5, 3600);
    state.changes += 1;
}

/// Set show weather.
pub fn set_show_weather(show: bool) {
    let mut state = STATE.lock();
    state.config.show_weather = show;
    state.changes += 1;
}

/// Set accessibility button visibility.
pub fn set_show_a11y(show: bool) {
    let mut state = STATE.lock();
    state.config.show_a11y = show;
    state.changes += 1;
}

/// Set show last login.
pub fn set_show_last_login(show: bool) {
    let mut state = STATE.lock();
    state.config.show_last_login = show;
    state.changes += 1;
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

/// Initialise with defaults.
pub fn init_defaults() {
    let mut state = STATE.lock();
    state.config = LoginConfig::default();
    state.changes += 1;
}

/// Return (synced, changes, ops).
pub fn stats() -> (bool, u64, u64) {
    let state = STATE.lock();
    (state.config.synced_with_desktop,
     state.changes,
     OP_COUNT.load(Ordering::Relaxed))
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.config = LoginConfig {
        background_mode: BackgroundMode::BlurDesktop,
        background_path: String::new(),
        background_color: String::new(),
        gradient_end: String::new(),
        fit_mode: FitMode::Fill,
        blur_amount: 30,
        slideshow_interval_s: 30,
        slideshow_dir: String::new(),
        synced_with_desktop: true,
        clock_position: ClockPosition::TopCenter,
        show_date: true,
        show_weather: false,
        user_list: UserListMode::ShowAll,
        show_last_login: true,
        virtual_keyboard: false,
        show_a11y: true,
        show_power: true,
        lock_timeout_s: 300,
        message: String::new(),
        logo_path: String::new(),
    };
    state.changes = 0;
    OP_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();

    // Test 1: defaults.
    serial_println!("loginscreen::self_test 1: defaults");
    init_defaults();
    let cfg = config();
    assert_eq!(cfg.background_mode, BackgroundMode::BlurDesktop);
    assert!(cfg.synced_with_desktop);
    assert_eq!(cfg.blur_amount, 30);

    // Test 2: set background image.
    serial_println!("loginscreen::self_test 2: background image");
    set_background_image("/usr/share/wallpapers/sunset.jpg");
    let cfg = config();
    assert_eq!(cfg.background_mode, BackgroundMode::Image);
    assert!(!cfg.synced_with_desktop);
    assert_eq!(cfg.background_path, "/usr/share/wallpapers/sunset.jpg");

    // Test 3: sync with desktop.
    serial_println!("loginscreen::self_test 3: sync");
    sync_with_desktop();
    let cfg = config();
    assert!(cfg.synced_with_desktop);
    assert_eq!(cfg.background_mode, BackgroundMode::BlurDesktop);

    // Test 4: appearance settings.
    serial_println!("loginscreen::self_test 4: appearance");
    set_background_mode(BackgroundMode::Gradient);
    set_background_color("2d2d44");
    set_gradient_end("1a1a2e");
    set_fit_mode(FitMode::Center);
    set_blur(50);
    let cfg = config();
    assert_eq!(cfg.background_mode, BackgroundMode::Gradient);
    assert_eq!(cfg.fit_mode, FitMode::Center);
    assert_eq!(cfg.blur_amount, 50);

    // Test 5: layout settings.
    serial_println!("loginscreen::self_test 5: layout");
    set_clock_position(ClockPosition::BottomRight);
    set_show_date(false);
    set_user_list(UserListMode::TextEntry);
    set_virtual_keyboard(true);
    set_show_power(false);
    set_lock_timeout(600);
    let cfg = config();
    assert_eq!(cfg.clock_position, ClockPosition::BottomRight);
    assert!(!cfg.show_date);
    assert_eq!(cfg.user_list, UserListMode::TextEntry);
    assert!(cfg.virtual_keyboard);
    assert!(!cfg.show_power);
    assert_eq!(cfg.lock_timeout_s, 600);

    // Test 6: message and logo.
    serial_println!("loginscreen::self_test 6: message and logo");
    set_message("Welcome to MintOS");
    set_logo("/usr/share/branding/logo.png");
    let cfg = config();
    assert_eq!(cfg.message, "Welcome to MintOS");
    assert_eq!(cfg.logo_path, "/usr/share/branding/logo.png");

    // Test 7: slideshow settings.
    serial_println!("loginscreen::self_test 7: slideshow");
    set_background_mode(BackgroundMode::Slideshow);
    set_slideshow_dir("/usr/share/wallpapers/");
    set_slideshow_interval(60);
    set_show_weather(true);
    set_show_a11y(false);
    set_show_last_login(false);
    let cfg = config();
    assert_eq!(cfg.background_mode, BackgroundMode::Slideshow);
    assert_eq!(cfg.slideshow_interval_s, 60);
    assert!(cfg.show_weather);
    assert!(!cfg.show_a11y);

    clear_all();
    serial_println!("loginscreen::self_test: all 7 tests passed");
    Ok(())
}
