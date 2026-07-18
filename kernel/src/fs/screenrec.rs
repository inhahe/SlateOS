//! Screen recording backend.
//!
//! Manages screen capture sessions for video recording.  The actual
//! pixel capture happens in the compositor; this module provides the
//! session lifecycle, settings, and file management.
//!
//! ## Architecture
//!
//! ```text
//! Keyboard shortcut / Settings panel
//!   → screenrec::start_recording()
//!   → screenrec::stop_recording()
//!
//! Compositor integration
//!   → screenrec::is_recording() — check if frames should be captured
//!   → screenrec::record_frame() — submit frame data
//!
//! Integration:
//!   → screenshot (still capture companion)
//!   → display (monitor selection)
//!   → soundmixer (audio capture settings)
//!   → hotkeys (record start/stop/pause hotkey)
//! ```
//!
//! ## Output Formats
//!
//! - WebM (VP9 + Opus)
//! - MP4 (H.264 + AAC)
//! - GIF (animated, no audio)

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_RECORDINGS: usize = 256;
const MAX_ACTIVE_SESSIONS: usize = 4;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Output format for recordings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    WebM,
    Mp4,
    Gif,
}

impl OutputFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::WebM => "WebM (VP9+Opus)",
            Self::Mp4 => "MP4 (H.264+AAC)",
            Self::Gif => "GIF (animated)",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::WebM => "webm",
            Self::Mp4 => "mp4",
            Self::Gif => "gif",
        }
    }
}

/// Audio capture mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioMode {
    /// No audio.
    None,
    /// System audio (desktop output).
    System,
    /// Microphone input.
    Microphone,
    /// Both system audio and microphone.
    Both,
}

impl AudioMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::System => "System Audio",
            Self::Microphone => "Microphone",
            Self::Both => "System + Microphone",
        }
    }
}

/// Capture area selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureArea {
    /// Full screen / all monitors.
    FullScreen,
    /// Single monitor by index.
    Monitor(u32),
    /// Specific window by ID.
    Window(u64),
    /// Custom region (x, y, width, height).
    Region,
}

impl CaptureArea {
    pub fn label(self) -> &'static str {
        match self {
            Self::FullScreen => "Full Screen",
            Self::Monitor(_) => "Single Monitor",
            Self::Window(_) => "Window",
            Self::Region => "Region",
        }
    }
}

/// Recording session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingState {
    /// Not recording.
    Idle,
    /// Pre-recording countdown.
    Countdown,
    /// Actively recording.
    Recording,
    /// Paused.
    Paused,
    /// Encoding/saving.
    Saving,
}

impl RecordingState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Countdown => "Countdown",
            Self::Recording => "Recording",
            Self::Paused => "Paused",
            Self::Saving => "Saving",
        }
    }
}

/// Recording quality preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityPreset {
    Low,
    Medium,
    High,
    Lossless,
}

impl QualityPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low (720p, 30fps)",
            Self::Medium => "Medium (1080p, 30fps)",
            Self::High => "High (1080p, 60fps)",
            Self::Lossless => "Lossless (native, 60fps)",
        }
    }

    pub fn fps(self) -> u32 {
        match self {
            Self::Low | Self::Medium => 30,
            Self::High | Self::Lossless => 60,
        }
    }
}

/// Configuration for recording sessions.
#[derive(Debug, Clone)]
pub struct RecordConfig {
    /// Output format.
    pub format: OutputFormat,
    /// Audio capture mode.
    pub audio: AudioMode,
    /// Quality preset.
    pub quality: QualityPreset,
    /// Capture area.
    pub area: CaptureArea,
    /// Custom region (x, y, w, h). Used when area == Region.
    pub region: (u32, u32, u32, u32),
    /// Frames per second (overrides preset).
    pub fps: u32,
    /// Show mouse cursor in recording.
    pub show_cursor: bool,
    /// Show click indicators.
    pub show_clicks: bool,
    /// Countdown seconds before recording starts (0 = immediate).
    pub countdown_seconds: u8,
    /// Maximum recording duration in seconds (0 = unlimited).
    pub max_duration_seconds: u32,
    /// Output directory.
    pub output_dir: String,
    /// Whether to show system tray indicator while recording.
    pub show_indicator: bool,
}

/// A completed or active recording session.
#[derive(Debug, Clone)]
pub struct Recording {
    /// Session ID.
    pub id: u64,
    /// State.
    pub state: RecordingState,
    /// Output file path.
    pub file_path: String,
    /// Format used.
    pub format: OutputFormat,
    /// Recording start time (ns).
    pub start_ns: u64,
    /// Recording end time (ns, 0 if still active).
    pub end_ns: u64,
    /// Duration in seconds.
    pub duration_seconds: u32,
    /// Frame count captured.
    pub frame_count: u64,
    /// File size in bytes (estimated or final).
    pub file_size: u64,
    /// Audio mode used.
    pub audio: AudioMode,
    /// Quality preset.
    pub quality: QualityPreset,
    /// FPS used.
    pub fps: u32,
    /// Capture area.
    pub area: CaptureArea,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct RecState {
    config: RecordConfig,
    recordings: Vec<Recording>,
    next_id: u64,
    total_recordings: u64,
    total_seconds: u64,
    total_bytes: u64,
    ops: u64,
}

static STATE: Mutex<Option<RecState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut RecState) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    let result = f(state)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the screen recording subsystem.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    *guard = Some(RecState {
        config: RecordConfig {
            format: OutputFormat::WebM,
            audio: AudioMode::System,
            quality: QualityPreset::High,
            area: CaptureArea::FullScreen,
            region: (0, 0, 1920, 1080),
            fps: 60,
            show_cursor: true,
            show_clicks: false,
            countdown_seconds: 3,
            max_duration_seconds: 0,
            output_dir: String::from("/home/Videos/Recordings"),
            show_indicator: true,
        },
        recordings: Vec::new(),
        next_id: 1,
        total_recordings: 0,
        total_seconds: 0,
        total_bytes: 0,
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Set the output format.
pub fn set_format(fmt: OutputFormat) -> KernelResult<()> {
    with_state(|state| {
        state.config.format = fmt;
        if fmt == OutputFormat::Gif {
            state.config.audio = AudioMode::None;
        }
        Ok(())
    })
}

/// Set the audio capture mode.
pub fn set_audio(mode: AudioMode) -> KernelResult<()> {
    with_state(|state| {
        if state.config.format == OutputFormat::Gif && mode != AudioMode::None {
            return Err(KernelError::NotSupported);
        }
        state.config.audio = mode;
        Ok(())
    })
}

/// Set the quality preset.
pub fn set_quality(q: QualityPreset) -> KernelResult<()> {
    with_state(|state| {
        state.config.quality = q;
        state.config.fps = q.fps();
        Ok(())
    })
}

/// Set the capture area.
pub fn set_area(area: CaptureArea) -> KernelResult<()> {
    with_state(|state| {
        state.config.area = area;
        Ok(())
    })
}

/// Set custom capture region.
pub fn set_region(x: u32, y: u32, w: u32, h: u32) -> KernelResult<()> {
    if w == 0 || h == 0 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.config.area = CaptureArea::Region;
        state.config.region = (x, y, w, h);
        Ok(())
    })
}

/// Set FPS (overrides preset).
pub fn set_fps(fps: u32) -> KernelResult<()> {
    if fps == 0 || fps > 240 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.config.fps = fps;
        Ok(())
    })
}

/// Set cursor visibility.
pub fn set_show_cursor(show: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.show_cursor = show;
        Ok(())
    })
}

/// Set click indicator visibility.
pub fn set_show_clicks(show: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.show_clicks = show;
        Ok(())
    })
}

/// Set countdown duration.
pub fn set_countdown(seconds: u8) -> KernelResult<()> {
    if seconds > 10 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.config.countdown_seconds = seconds;
        Ok(())
    })
}

/// Set maximum recording duration.
pub fn set_max_duration(seconds: u32) -> KernelResult<()> {
    with_state(|state| {
        state.config.max_duration_seconds = seconds;
        Ok(())
    })
}

/// Set output directory.
pub fn set_output_dir(dir: &str) -> KernelResult<()> {
    if dir.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.config.output_dir = String::from(dir);
        Ok(())
    })
}

/// Get current config.
pub fn get_config() -> KernelResult<RecordConfig> {
    let guard = STATE.lock();
    guard.as_ref()
        .map(|s| s.config.clone())
        .ok_or(KernelError::NotSupported)
}

// ---------------------------------------------------------------------------
// Recording lifecycle
// ---------------------------------------------------------------------------

/// Start a new recording session. Returns session ID.
pub fn start_recording() -> KernelResult<u64> {
    with_state(|state| {
        let active = state.recordings.iter()
            .filter(|r| matches!(r.state,
                RecordingState::Recording | RecordingState::Paused | RecordingState::Countdown))
            .count();
        if active >= MAX_ACTIVE_SESSIONS {
            return Err(KernelError::ResourceExhausted);
        }
        if state.recordings.len() >= MAX_RECORDINGS {
            // Remove oldest completed.
            if let Some(pos) = state.recordings.iter().position(|r| r.state == RecordingState::Idle) {
                state.recordings.remove(pos);
            } else {
                return Err(KernelError::ResourceExhausted);
            }
        }

        let id = state.next_id;
        state.next_id += 1;
        let now = crate::hpet::elapsed_ns();

        let file_name = format!("recording_{}.{}", id, state.config.format.extension());
        let file_path = format!("{}/{}", state.config.output_dir, file_name);

        let initial_state = if state.config.countdown_seconds > 0 {
            RecordingState::Countdown
        } else {
            RecordingState::Recording
        };

        state.recordings.push(Recording {
            id,
            state: initial_state,
            file_path,
            format: state.config.format,
            start_ns: now,
            end_ns: 0,
            duration_seconds: 0,
            frame_count: 0,
            file_size: 0,
            audio: state.config.audio,
            quality: state.config.quality,
            fps: state.config.fps,
            area: state.config.area,
        });

        state.total_recordings += 1;
        Ok(id)
    })
}

/// Transition from countdown to recording.
pub fn begin_capture(id: u64) -> KernelResult<()> {
    with_state(|state| {
        let rec = state.recordings.iter_mut()
            .find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        if rec.state != RecordingState::Countdown {
            return Err(KernelError::InvalidArgument);
        }
        rec.state = RecordingState::Recording;
        rec.start_ns = crate::hpet::elapsed_ns();
        Ok(())
    })
}

/// Pause recording.
pub fn pause_recording(id: u64) -> KernelResult<()> {
    with_state(|state| {
        let rec = state.recordings.iter_mut()
            .find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        if rec.state != RecordingState::Recording {
            return Err(KernelError::InvalidArgument);
        }
        rec.state = RecordingState::Paused;
        Ok(())
    })
}

/// Resume recording.
pub fn resume_recording(id: u64) -> KernelResult<()> {
    with_state(|state| {
        let rec = state.recordings.iter_mut()
            .find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        if rec.state != RecordingState::Paused {
            return Err(KernelError::InvalidArgument);
        }
        rec.state = RecordingState::Recording;
        Ok(())
    })
}

/// Stop recording and finalize.
pub fn stop_recording(id: u64) -> KernelResult<Recording> {
    with_state(|state| {
        let rec = state.recordings.iter_mut()
            .find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        if !matches!(rec.state, RecordingState::Recording | RecordingState::Paused | RecordingState::Countdown) {
            return Err(KernelError::InvalidArgument);
        }

        let now = crate::hpet::elapsed_ns();
        rec.end_ns = now;
        rec.duration_seconds = ((now.saturating_sub(rec.start_ns)) / 1_000_000_000) as u32;
        rec.state = RecordingState::Idle;

        // Estimate file size based on duration and quality.
        let bytes_per_sec: u64 = match rec.quality {
            QualityPreset::Low => 500_000,      // ~500KB/s
            QualityPreset::Medium => 2_000_000,  // ~2MB/s
            QualityPreset::High => 5_000_000,    // ~5MB/s
            QualityPreset::Lossless => 20_000_000, // ~20MB/s
        };
        rec.file_size = bytes_per_sec * rec.duration_seconds as u64;

        state.total_seconds += rec.duration_seconds as u64;
        state.total_bytes += rec.file_size;

        Ok(rec.clone())
    })
}

/// Record a frame (called by compositor).
pub fn record_frame(id: u64) -> KernelResult<()> {
    with_state(|state| {
        let rec = state.recordings.iter_mut()
            .find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        if rec.state != RecordingState::Recording {
            return Err(KernelError::InvalidArgument);
        }
        rec.frame_count += 1;
        Ok(())
    })
}

/// Check if any session is actively recording.
pub fn is_recording() -> bool {
    let guard = STATE.lock();
    guard.as_ref().is_some_and(|s| {
        s.recordings.iter().any(|r| r.state == RecordingState::Recording)
    })
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Get a recording by ID.
pub fn get_recording(id: u64) -> KernelResult<Recording> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    state.recordings.iter()
        .find(|r| r.id == id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all recordings.
pub fn list_recordings() -> Vec<Recording> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.recordings.clone())
}

/// List active recordings.
pub fn active_recordings() -> Vec<Recording> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.recordings.iter()
            .filter(|r| matches!(r.state,
                RecordingState::Recording | RecordingState::Paused | RecordingState::Countdown))
            .cloned()
            .collect()
    })
}

/// Remove a completed recording from the list.
pub fn remove_recording(id: u64) -> KernelResult<()> {
    with_state(|state| {
        if let Some(pos) = state.recordings.iter().position(|r| r.id == id) {
            if state.recordings[pos].state != RecordingState::Idle {
                return Err(KernelError::InvalidArgument);
            }
            state.recordings.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (recording_count, active_count, total_recordings, total_seconds, total_bytes, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let active = s.recordings.iter()
                .filter(|r| matches!(r.state, RecordingState::Recording | RecordingState::Paused))
                .count();
            (s.recordings.len(), active, s.total_recordings, s.total_seconds, s.total_bytes, s.ops)
        }
        None => (0, 0, 0, 0, 0, 0),
    }
}

/// Format file size as human-readable string.
fn format_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{}.{} GB", bytes / 1_073_741_824, (bytes % 1_073_741_824) / 107_374_182)
    } else if bytes >= 1_048_576 {
        format!("{}.{} MB", bytes / 1_048_576, (bytes % 1_048_576) / 104_857)
    } else if bytes >= 1024 {
        format!("{}.{} KB", bytes / 1024, (bytes % 1024) / 102)
    } else {
        format!("{} B", bytes)
    }
}

/// Format duration as human-readable.
pub fn format_duration(seconds: u32) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    if h > 0 {
        format!("{}h {}m {}s", h, m, s)
    } else if m > 0 {
        format!("{}m {}s", m, s)
    } else {
        format!("{}s", s)
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the screen recording module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[screenrec] Running self-tests...");

    // Reset state.
    *STATE.lock() = None;
    init_defaults();

    // Test 1: initial state.
    {
        let (count, active, _, _, _, _) = stats();
        assert_eq!(count, 0);
        assert_eq!(active, 0);
        assert!(!is_recording());
    }
    serial_println!("[screenrec]  1/11 initial state OK");

    // Test 2: config defaults.
    {
        let cfg = get_config().unwrap();
        assert_eq!(cfg.format, OutputFormat::WebM);
        assert_eq!(cfg.audio, AudioMode::System);
        assert_eq!(cfg.fps, 60);
        assert!(cfg.show_cursor);
    }
    serial_println!("[screenrec]  2/11 config defaults OK");

    // Test 3: set format.
    {
        set_format(OutputFormat::Mp4).unwrap();
        let cfg = get_config().unwrap();
        assert_eq!(cfg.format, OutputFormat::Mp4);

        set_format(OutputFormat::Gif).unwrap();
        let cfg = get_config().unwrap();
        assert_eq!(cfg.audio, AudioMode::None);
        set_format(OutputFormat::WebM).unwrap();
    }
    serial_println!("[screenrec]  3/11 set format OK");

    // Test 4: set quality.
    {
        set_quality(QualityPreset::Low).unwrap();
        let cfg = get_config().unwrap();
        assert_eq!(cfg.fps, 30);
        set_quality(QualityPreset::High).unwrap();
    }
    serial_println!("[screenrec]  4/11 set quality OK");

    // Test 5: start recording.
    {
        set_countdown(0).unwrap();
        let id = start_recording().unwrap();
        assert!(id > 0);
        assert!(is_recording());
        let rec = get_recording(id).unwrap();
        assert_eq!(rec.state, RecordingState::Recording);
        // Clean up.
        let _ = stop_recording(id);
    }
    serial_println!("[screenrec]  5/11 start recording OK");

    // Test 6: pause/resume.
    {
        let id = start_recording().unwrap();
        pause_recording(id).unwrap();
        let rec = get_recording(id).unwrap();
        assert_eq!(rec.state, RecordingState::Paused);
        assert!(!is_recording()); // Paused != recording.

        resume_recording(id).unwrap();
        assert!(is_recording());
        let _ = stop_recording(id);
    }
    serial_println!("[screenrec]  6/11 pause/resume OK");

    // Test 7: stop recording.
    {
        let id = start_recording().unwrap();
        record_frame(id).unwrap();
        record_frame(id).unwrap();
        let result = stop_recording(id).unwrap();
        assert_eq!(result.state, RecordingState::Idle);
        assert_eq!(result.frame_count, 2);
        assert!(!is_recording());
    }
    serial_println!("[screenrec]  7/11 stop recording OK");

    // Test 8: countdown.
    {
        set_countdown(3).unwrap();
        let id = start_recording().unwrap();
        let rec = get_recording(id).unwrap();
        assert_eq!(rec.state, RecordingState::Countdown);
        begin_capture(id).unwrap();
        let rec = get_recording(id).unwrap();
        assert_eq!(rec.state, RecordingState::Recording);
        let _ = stop_recording(id);
        set_countdown(0).unwrap();
    }
    serial_println!("[screenrec]  8/11 countdown OK");

    // Test 9: region capture.
    {
        set_region(100, 200, 800, 600).unwrap();
        let cfg = get_config().unwrap();
        assert_eq!(cfg.area, CaptureArea::Region);
        assert_eq!(cfg.region, (100, 200, 800, 600));
        assert!(set_region(0, 0, 0, 0).is_err());
        set_area(CaptureArea::FullScreen).unwrap();
    }
    serial_println!("[screenrec]  9/11 region capture OK");

    // Test 10: remove recording.
    {
        let id = start_recording().unwrap();
        let _ = stop_recording(id);
        remove_recording(id).unwrap();
        assert!(get_recording(id).is_err());
    }
    serial_println!("[screenrec] 10/11 remove recording OK");

    // Test 11: stats.
    {
        let (_, _, total, _, _, _) = stats();
        assert!(total > 0);
    }
    serial_println!("[screenrec] 11/11 stats OK");

    serial_println!("[screenrec] All self-tests passed.");
}
