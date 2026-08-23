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

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};

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
    pub path: PathBuf,
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
    pub save_dir: PathBuf,
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
    /// Filename stem prefix, e.g. `screenshot` -> `screenshot_7.png`.
    ///
    /// Raw bytes rather than `String`: this is a filename component, and a
    /// filename may hold any byte but `/` and NUL (design-decisions.md §261).
    /// A user whose locale spells the word with bytes that are not valid UTF-8
    /// would otherwise get U+FFFD baked into every screenshot they take.
    pub filename_pattern: Vec<u8>,
}

impl Default for ScreenshotConfig {
    fn default() -> Self {
        Self {
            save_dir: PathBuf::from("/home/user/Pictures/Screenshots"),
            format: ImageFormat::Png,
            jpeg_quality: 85,
            include_cursor: false,
            play_sound: true,
            show_flash: true,
            copy_to_clipboard: true,
            open_after_capture: false,
            delay_seconds: 0,
            filename_pattern: b"screenshot".to_vec(),
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
                save_dir: PathBuf::new(),
                format: ImageFormat::Png,
                jpeg_quality: 85,
                include_cursor: false,
                play_sound: true,
                show_flash: true,
                copy_to_clipboard: true,
                open_after_capture: false,
                delay_seconds: 0,
                filename_pattern: Vec::new(),
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
    record_capture(
        CaptureKind::Window,
        width,
        height,
        Some(window_id),
        None,
        None,
    )
}

/// Capture a rectangular region.
pub fn capture_region(x: u32, y: u32, w: u32, h: u32) -> KernelResult<u64> {
    record_capture(CaptureKind::Region, w, h, None, None, Some((x, y, w, h)))
}

/// Capture a specific monitor.
pub fn capture_monitor(monitor_id: &str, width: u32, height: u32) -> KernelResult<u64> {
    record_capture(
        CaptureKind::Monitor,
        width,
        height,
        None,
        Some(monitor_id),
        None,
    )
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
    //
    // `join` rather than `format!("{}/{}", dir, name)`: the save directory may
    // hold bytes with no UTF-8 spelling (design-decisions.md §261), which a
    // formatted concatenation cannot carry, and `join` additionally collapses a
    // trailing `/` that the concatenation would have doubled.
    // The configured stem, or `screenshot` when unset.  `clear_all`/`State::new`
    // leave it empty, and an empty stem would produce the bare name `_7.png`.
    let mut name = if state.config.filename_pattern.is_empty() {
        b"screenshot".to_vec()
    } else {
        state.config.filename_pattern.clone()
    };
    name.extend_from_slice(alloc::format!("_{}.{}", id, state.config.format.label()).as_bytes());
    let name = PathBuf::from_vec(name);
    let path = if state.config.save_dir.is_empty() {
        Path::new("/tmp").join(&name)
    } else {
        state.config.save_dir.join(&name)
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
pub fn set_save_dir(dir: impl AsRef<Path>) -> KernelResult<()> {
    let dir = dir.as_ref();
    if dir.len() > MAX_PATH_LEN {
        return Err(KernelError::InvalidArgument);
    }
    STATE.lock().config.save_dir = dir.to_path_buf();
    Ok(())
}

/// Set the filename stem prefix, e.g. `holiday` -> `holiday_7.png`.
///
/// Rejects a pattern containing `/` or NUL. This is a *filename component*,
/// not a path: allowing `/` would let a "pattern" such as `../../etc/passwd`
/// steer a capture out of the configured save directory, and NUL cannot appear
/// in a name at all. An empty pattern restores the `screenshot` default.
///
/// # Errors
///
/// `InvalidArgument` if the pattern is longer than `MAX_PATH_LEN` or contains
/// a separator or NUL byte.
pub fn set_filename_pattern(pattern: impl AsRef<[u8]>) -> KernelResult<()> {
    let pattern = pattern.as_ref();
    if pattern.len() > MAX_PATH_LEN {
        return Err(KernelError::InvalidArgument);
    }
    if pattern.iter().any(|&b| b == b'/' || b == 0) {
        return Err(KernelError::InvalidArgument);
    }
    STATE.lock().config.filename_pattern = pattern.to_vec();
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
        save_dir: PathBuf::new(),
        format: ImageFormat::Png,
        jpeg_quality: 85,
        include_cursor: false,
        play_sound: true,
        show_flash: true,
        copy_to_clipboard: true,
        open_after_capture: false,
        delay_seconds: 0,
        filename_pattern: Vec::new(),
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
    assert_eq!(cfg.save_dir.as_path(), Path::new("/home/user/Screenshots"));
    assert_eq!(cfg.format, ImageFormat::Jpeg);
    assert_eq!(cfg.jpeg_quality, 90);
    assert!(cfg.include_cursor);
    assert_eq!(cfg.delay_seconds, 3);

    // Capture with new format.
    let id5 = capture_full(3840, 2160)?;
    let s5 = get(id5).unwrap();
    assert_eq!(s5.format, ImageFormat::Jpeg);
    // `extension()` rather than a substring search: the old `contains(".jpeg")`
    // would also have accepted a save directory that merely had `.jpeg` in its
    // name, and there is no `contains` on a byte path to inherit anyway.
    assert_eq!(s5.path.as_path().extension(), Some(Path::new("jpeg")));
    assert_eq!(
        s5.path.as_path().parent(),
        Some(Path::new("/home/user/Screenshots"))
    );

    // Test 7: Delete and stats.
    serial_println!("  screenshot::self_test 7: delete and stats");
    delete(id1)?;
    assert!(get(id1).is_none());
    assert_eq!(history().len(), 4);
    let (hc, cc) = stats();
    assert_eq!(hc, 4);
    assert_eq!(cc, 5);

    // Test 8: Non-UTF-8 save directory (design-decisions.md §261).
    //
    // `\xFF` and `\xFE` are both invalid as a UTF-8 leading byte, so under the
    // old `String` typing both directories below collapsed to the same
    // U+FFFD-bearing name -- and the screenshot's recorded path, which is what
    // the "open after capture" action and the history listing both use, would
    // then point at a directory that does not exist.
    serial_println!("  screenshot::self_test 8: non-UTF-8 save directory");
    let dir_a = Path::new(&b"/home/user/ss_\xFFshots"[..]);
    let dir_b = Path::new(&b"/home/user/ss_\xFEshots"[..]);
    set_save_dir(dir_a)?;
    assert_eq!(config().save_dir.as_path(), dir_a);
    assert_ne!(
        config().save_dir.as_path(),
        dir_b,
        "\\xFF must not fold to \\xFE"
    );
    set_format(ImageFormat::Png);
    let id6 = capture_full(640, 480)?;
    let s6 = get(id6).ok_or(KernelError::NotFound)?;
    assert_eq!(s6.path.as_path().parent(), Some(dir_a));
    let mut expected = b"/home/user/ss_\xFFshots/screenshot_".to_vec();
    expected.extend_from_slice(alloc::format!("{id6}.png").as_bytes());
    assert_eq!(
        s6.path.as_path().as_bytes(),
        &expected[..],
        "the save path must carry the directory's bytes through unchanged"
    );

    // A save directory the user typed with a trailing `/` must not yield a
    // doubled separator.  This is what `join` buys over the `format!` the
    // path build used to do; assert it so a revert cannot pass silently.
    set_save_dir(Path::new(&b"/home/user/ss_\xFFshots/"[..]))?;
    let id7 = capture_full(320, 240)?;
    let s7 = get(id7).ok_or(KernelError::NotFound)?;
    assert_eq!(s7.path.as_path().parent(), Some(dir_a));
    assert!(
        !s7.path.as_path().as_bytes().windows(2).any(|w| w == b"//"),
        "join must collapse the trailing separator"
    );

    // Test 9: Filename pattern.
    //
    // The pattern was pure dead config until this was wired up: it had no
    // setter and `record_capture` hard-coded the stem `screenshot`, so the
    // documented "e.g. screenshot_%Y%m%d" was a promise nothing kept.
    serial_println!("  screenshot::self_test 9: filename pattern");
    set_save_dir("/tmp/ss")?;
    set_filename_pattern("holiday")?;
    let id8 = capture_full(100, 100)?;
    let s8 = get(id8).ok_or(KernelError::NotFound)?;
    assert_eq!(
        s8.path.as_path(),
        Path::new(&alloc::format!("/tmp/ss/holiday_{id8}.png"))
    );

    // A pattern is a name, so it is byte-typed like every other name here.
    set_filename_pattern(&b"ur\xFFlaub"[..])?;
    let id9 = capture_full(100, 100)?;
    let s9 = get(id9).ok_or(KernelError::NotFound)?;
    let mut want = b"/tmp/ss/ur\xFFlaub_".to_vec();
    want.extend_from_slice(alloc::format!("{id9}.png").as_bytes());
    assert_eq!(s9.path.as_path().as_bytes(), &want[..]);

    // A pattern is a *component*: `/` would let it escape the save directory,
    // and NUL cannot appear in a name at all.  Both must be refused, and a
    // refusal must not have disturbed the pattern already in place.
    assert!(set_filename_pattern("../../etc/passwd").is_err());
    assert!(set_filename_pattern("a/b").is_err());
    assert!(set_filename_pattern(&b"nul\0byte"[..]).is_err());
    assert_eq!(config().filename_pattern, b"ur\xFFlaub".to_vec());

    // An empty pattern falls back to the default stem rather than producing
    // the bare name `_10.png`.
    set_filename_pattern("")?;
    let id10 = capture_full(100, 100)?;
    let s10 = get(id10).ok_or(KernelError::NotFound)?;
    assert_eq!(
        s10.path.as_path(),
        Path::new(&alloc::format!("/tmp/ss/screenshot_{id10}.png"))
    );

    clear_all();
    reset_stats();
    serial_println!("  screenshot: all tests passed");
    Ok(())
}
