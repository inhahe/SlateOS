//! Screenshot / screen capture — framebuffer capture and saving.
//!
//! Captures the desktop framebuffer contents for screenshots and
//! screen recording triggers.  Works with the compositor to grab
//! frame data.
//!
//! ## Design Reference
//!
//! design.txt line 1327: "Print Screen for screenshot"
//! design.txt line 1087: compositor screen capture API
//!
//! ## Architecture
//!
//! ```text
//! Hotkey (PrintScreen)
//!   → screenshot::capture_full()
//!
//! Hotkey (Alt+PrintScreen)
//!   → screenshot::capture_window(window_id)
//!
//! Hotkey (Ctrl+Shift+PrintScreen)
//!   → screenshot::capture_region(x, y, w, h)
//!
//! All captures → save to configured directory
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum stored screenshots (metadata only; actual pixels would be in files).
const MAX_HISTORY: usize = 256;

/// Maximum save directory path length.
const MAX_PATH_LEN: usize = 512;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// What was captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureKind {
    /// Entire screen / all monitors.
    FullScreen,
    /// A specific window.
    Window,
    /// A user-selected rectangular region.
    Region,
    /// A specific monitor.
    Monitor,
}

impl CaptureKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::FullScreen => "fullscreen",
            Self::Window => "window",
            Self::Region => "region",
            Self::Monitor => "monitor",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "full" | "fullscreen" | "screen" => Some(Self::FullScreen),
            "window" | "win" => Some(Self::Window),
            "region" | "rect" => Some(Self::Region),
            "monitor" | "mon" => Some(Self::Monitor),
            _ => None,
        }
    }
}

/// Image output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Bmp,
    Webp,
}

impl ImageFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Bmp => "bmp",
            Self::Webp => "webp",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => ".png",
            Self::Jpeg => ".jpg",
            Self::Bmp => ".bmp",
            Self::Webp => ".webp",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "png" => Some(Self::Png),
            "jpeg" | "jpg" => Some(Self::Jpeg),
            "bmp" => Some(Self::Bmp),
            "webp" => Some(Self::Webp),
            _ => None,
        }
    }
}

/// Metadata for a captured screenshot.
#[derive(Debug, Clone)]
pub struct Screenshot {
    /// Unique capture ID.
    pub id: u64,
    /// Capture kind.
    pub kind: CaptureKind,
    /// Timestamp (monotonic nanoseconds).
    pub timestamp_ns: u64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Saved file path (if saved).
    pub path: String,
    /// Image format.
    pub format: ImageFormat,
    /// Window ID (if CaptureKind::Window).
    pub window_id: Option<u64>,
    /// Monitor ID (if CaptureKind::Monitor).
    pub monitor_id: Option<String>,
    /// Region coordinates (if CaptureKind::Region).
    pub region: Option<(u32, u32, u32, u32)>,
}

/// Configuration for the screenshot system.
#[derive(Debug, Clone)]
pub struct ScreenshotConfig {
    /// Directory where screenshots are saved.
    pub save_dir: String,
    /// Default image format.
    pub format: ImageFormat,
    /// JPEG quality (1-100).
    pub jpeg_quality: u8,
    /// Whether to include the mouse cursor in captures.
    pub include_cursor: bool,
    /// Whether to play a shutter sound effect.
    pub play_sound: bool,
    /// Whether to show a flash animation.
    pub show_flash: bool,
    /// Whether to copy to clipboard automatically.
    pub copy_to_clipboard: bool,
    /// Whether to open the screenshot after capture.
    pub open_after_capture: bool,
    /// Delay before capture (seconds, 0 = immediate).
    pub delay_seconds: u32,
    /// Filename pattern (e.g., "screenshot_%Y%m%d_%H%M%S").
    pub filename_pattern: String,
}

impl Default for ScreenshotConfig {
    fn default() -> Self {
        Self {
            save_dir: String::from("/home/user/Pictures/Screenshots"),
            format: ImageFormat::Png,
            jpeg_quality: 85,
            include_cursor: false,
            play_sound: true,
            show_flash: true,
            copy_to_clipboard: true,
            open_after_capture: false,
            delay_seconds: 0,
            filename_pattern: String::from("screenshot"),
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct State {
    config: ScreenshotConfig,
    history: Vec<Screenshot>,
    next_id: u64,
}

impl State {
    const fn new() -> Self {
        Self {
            config: ScreenshotConfig {
                save_dir: String::new(),
                format: ImageFormat::Png,
                jpeg_quality: 85,
                include_cursor: false,
                play_sound: true,
                show_flash: true,
                copy_to_clipboard: true,
                open_after_capture: false,
                delay_seconds: 0,
                filename_pattern: String::new(),
            },
            history: Vec::new(),
            next_id: 1,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());
static CAPTURE_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

/// Initialize with default config.
pub fn init_defaults() {
    let mut state = STATE.lock();
    state.config = ScreenshotConfig::default();
}

// ---------------------------------------------------------------------------
// Capture operations
// ---------------------------------------------------------------------------

/// Capture the full screen (all monitors).
///
/// In a real system this would ask the compositor for framebuffer data.
/// Here we record the capture metadata.
pub fn capture_full(width: u32, height: u32) -> KernelResult<u64> {
    record_capture(CaptureKind::FullScreen, width, height, None, None, None)
}

/// Capture a specific window.
pub fn capture_window(window_id: u64, width: u32, height: u32) -> KernelResult<u64> {
    record_capture(CaptureKind::Window, width, height, Some(window_id), None, None)
}

/// Capture a rectangular region.
pub fn capture_region(x: u32, y: u32, w: u32, h: u32) -> KernelResult<u64> {
    record_capture(CaptureKind::Region, w, h, None, None, Some((x, y, w, h)))
}

/// Capture a specific monitor.
pub fn capture_monitor(monitor_id: &str, width: u32, height: u32) -> KernelResult<u64> {
    record_capture(CaptureKind::Monitor, width, height, None, Some(monitor_id), None)
}

fn record_capture(
    kind: CaptureKind,
    width: u32,
    height: u32,
    window_id: Option<u64>,
    monitor_id: Option<&str>,
    region: Option<(u32, u32, u32, u32)>,
) -> KernelResult<u64> {
    let mut state = STATE.lock();
    let id = state.next_id;
    state.next_id = state.next_id.wrapping_add(1);

    let ns = crate::hpet::elapsed_ns();

    // Build the save path.
    let path = if state.config.save_dir.is_empty() {
        alloc::format!("/tmp/screenshot_{}.{}", id, state.config.format.label())
    } else {
        alloc::format!("{}/screenshot_{}.{}",
            state.config.save_dir, id, state.config.format.label())
    };

    let screenshot = Screenshot {
        id,
        kind,
        timestamp_ns: ns,
        width,
        height,
        path,
        format: state.config.format,
        window_id,
        monitor_id: monitor_id.map(String::from),
        region,
    };

    // Maintain bounded history.
    if state.history.len() >= MAX_HISTORY {
        state.history.remove(0);
    }
    state.history.push(screenshot);
    CAPTURE_COUNT.fetch_add(1, Ordering::Relaxed);

    Ok(id)
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

/// Get a screenshot by ID.
pub fn get(id: u64) -> Option<Screenshot> {
    STATE.lock().history.iter().find(|s| s.id == id).cloned()
}

/// List recent screenshots (newest first).
pub fn recent(count: usize) -> Vec<Screenshot> {
    let state = STATE.lock();
    let mut result: Vec<Screenshot> = state.history.iter().rev().take(count).cloned().collect();
    result.reverse(); // Oldest first within the returned slice.
    result
}

/// List all screenshots.
pub fn history() -> Vec<Screenshot> {
    STATE.lock().history.clone()
}

/// Clear screenshot history.
pub fn clear_history() {
    STATE.lock().history.clear();
}

/// Delete a screenshot from history.
pub fn delete(id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let len_before = state.history.len();
    state.history.retain(|s| s.id != id);
    if state.history.len() == len_before {
        return Err(KernelError::NotFound);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Get current config.
pub fn config() -> ScreenshotConfig {
    STATE.lock().config.clone()
}

/// Set save directory.
pub fn set_save_dir(dir: &str) -> KernelResult<()> {
    if dir.len() > MAX_PATH_LEN {
        return Err(KernelError::InvalidArgument);
    }
    STATE.lock().config.save_dir = String::from(dir);
    Ok(())
}

/// Set default image format.
pub fn set_format(fmt: ImageFormat) {
    STATE.lock().config.format = fmt;
}

/// Set JPEG quality.
pub fn set_jpeg_quality(q: u8) {
    STATE.lock().config.jpeg_quality = q.clamp(1, 100);
}

/// Set cursor inclusion.
pub fn set_include_cursor(v: bool) {
    STATE.lock().config.include_cursor = v;
}

/// Set shutter sound.
pub fn set_play_sound(v: bool) {
    STATE.lock().config.play_sound = v;
}

/// Set flash animation.
pub fn set_show_flash(v: bool) {
    STATE.lock().config.show_flash = v;
}

/// Set clipboard auto-copy.
pub fn set_copy_to_clipboard(v: bool) {
    STATE.lock().config.copy_to_clipboard = v;
}

/// Set open-after-capture.
pub fn set_open_after(v: bool) {
    STATE.lock().config.open_after_capture = v;
}

/// Set capture delay in seconds.
pub fn set_delay(seconds: u32) {
    STATE.lock().config.delay_seconds = seconds;
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Returns (history_count, capture_count).
pub fn stats() -> (usize, u64) {
    let state = STATE.lock();
    (state.history.len(), CAPTURE_COUNT.load(Ordering::Relaxed))
}

/// Reset stats.
pub fn reset_stats() {
    CAPTURE_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all state.
pub fn clear_all() {
    let mut state = STATE.lock();
    state.history.clear();
    state.config = ScreenshotConfig {
        save_dir: String::new(),
        format: ImageFormat::Png,
        jpeg_quality: 85,
        include_cursor: false,
        play_sound: true,
        show_flash: true,
        copy_to_clipboard: true,
        open_after_capture: false,
        delay_seconds: 0,
        filename_pattern: String::new(),
    };
    state.next_id = 1;
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;
    clear_all();
    reset_stats();

    // Test 1: Full screen capture.
    serial_println!("  screenshot::self_test 1: full screen capture");
    let id1 = capture_full(1920, 1080)?;
    assert!(id1 > 0);
    let s = get(id1);
    assert!(s.is_some());
    let s = s.unwrap();
    assert_eq!(s.kind, CaptureKind::FullScreen);
    assert_eq!(s.width, 1920);
    assert_eq!(s.height, 1080);

    // Test 2: Window capture.
    serial_println!("  screenshot::self_test 2: window capture");
    let id2 = capture_window(42, 800, 600)?;
    let s2 = get(id2).unwrap();
    assert_eq!(s2.kind, CaptureKind::Window);
    assert_eq!(s2.window_id, Some(42));

    // Test 3: Region capture.
    serial_println!("  screenshot::self_test 3: region capture");
    let id3 = capture_region(100, 200, 400, 300)?;
    let s3 = get(id3).unwrap();
    assert_eq!(s3.kind, CaptureKind::Region);
    assert_eq!(s3.region, Some((100, 200, 400, 300)));

    // Test 4: Monitor capture.
    serial_println!("  screenshot::self_test 4: monitor capture");
    let id4 = capture_monitor("HDMI-1", 2560, 1440)?;
    let s4 = get(id4).unwrap();
    assert_eq!(s4.kind, CaptureKind::Monitor);
    assert_eq!(s4.monitor_id.as_deref(), Some("HDMI-1"));

    // Test 5: History.
    serial_println!("  screenshot::self_test 5: history");
    assert_eq!(history().len(), 4);
    let r = recent(2);
    assert_eq!(r.len(), 2);

    // Test 6: Configuration.
    serial_println!("  screenshot::self_test 6: config");
    set_save_dir("/home/user/Screenshots")?;
    set_format(ImageFormat::Jpeg);
    set_jpeg_quality(90);
    set_include_cursor(true);
    set_delay(3);
    let cfg = config();
    assert_eq!(cfg.save_dir, "/home/user/Screenshots");
    assert_eq!(cfg.format, ImageFormat::Jpeg);
    assert_eq!(cfg.jpeg_quality, 90);
    assert!(cfg.include_cursor);
    assert_eq!(cfg.delay_seconds, 3);

    // Capture with new format.
    let id5 = capture_full(3840, 2160)?;
    let s5 = get(id5).unwrap();
    assert_eq!(s5.format, ImageFormat::Jpeg);
    assert!(s5.path.contains(".jpeg"));

    // Test 7: Delete and stats.
    serial_println!("  screenshot::self_test 7: delete and stats");
    delete(id1)?;
    assert!(get(id1).is_none());
    assert_eq!(history().len(), 4);
    let (hc, cc) = stats();
    assert_eq!(hc, 4);
    assert_eq!(cc, 5);

    clear_all();
    reset_stats();
    serial_println!("  screenshot: all tests passed");
    Ok(())
}
