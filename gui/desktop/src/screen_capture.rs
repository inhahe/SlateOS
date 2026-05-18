//! Screen Recording / Capture Module
//!
//! Desktop-level screen recording infrastructure:
//!
//! - Region selection for recording area
//! - Full-screen, window, or custom rectangle capture
//! - Frame rate configuration (15/30/60 fps)
//! - Audio capture toggle (system + microphone)
//! - Recording timer with pause/resume
//! - Output format selection (raw frames → container format)
//! - Cursor visibility toggle
//! - Countdown timer before recording starts
//! - Hotkey integration (start/stop/pause)
//! - Recording indicator overlay

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const MOCHA_BASE: Color = Color::from_hex(0x1E1E2E);
const MOCHA_MANTLE: Color = Color::from_hex(0x181825);
const MOCHA_SURFACE0: Color = Color::from_hex(0x313244);
const MOCHA_SURFACE1: Color = Color::from_hex(0x45475A);
const MOCHA_TEXT: Color = Color::from_hex(0xCDD6F4);
const MOCHA_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const MOCHA_BLUE: Color = Color::from_hex(0x89B4FA);
const MOCHA_GREEN: Color = Color::from_hex(0xA6E3A1);
const MOCHA_RED: Color = Color::from_hex(0xF38BA8);
const MOCHA_YELLOW: Color = Color::from_hex(0xF9E2AF);
const MOCHA_PEACH: Color = Color::from_hex(0xFAB387);
const MOCHA_OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Capture mode
// ============================================================================

/// What region of the screen to capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureMode {
    /// Entire primary display.
    FullScreen,
    /// A specific monitor (0-based index).
    Monitor(u32),
    /// A specific window (by window ID).
    Window(u64),
    /// A user-selected rectangle.
    Region,
}

impl CaptureMode {
    pub fn label(&self) -> &str {
        match self {
            Self::FullScreen => "Full Screen",
            Self::Monitor(_) => "Monitor",
            Self::Window(_) => "Window",
            Self::Region => "Region",
        }
    }
}

/// The selected rectangular region for recording.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaptureRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl CaptureRegion {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    /// Calculate the total pixel count.
    pub fn pixel_count(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Estimate raw frame size in bytes (BGRA = 4 bytes per pixel).
    pub fn frame_size_bytes(&self) -> u64 {
        self.pixel_count() * 4
    }

    /// Check if the region is valid (non-zero dimensions).
    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0
    }
}

// ============================================================================
// Frame rate
// ============================================================================

/// Target frame rate for recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameRate {
    Fps15,
    Fps30,
    Fps60,
}

impl FrameRate {
    pub fn value(&self) -> u32 {
        match self {
            Self::Fps15 => 15,
            Self::Fps30 => 30,
            Self::Fps60 => 60,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Fps15 => "15 fps",
            Self::Fps30 => "30 fps",
            Self::Fps60 => "60 fps",
        }
    }

    /// Estimated raw data rate in MB/s for a given region.
    pub fn data_rate_mbps(&self, region: &CaptureRegion) -> f64 {
        (region.frame_size_bytes() as f64 * self.value() as f64) / (1024.0 * 1024.0)
    }
}

// ============================================================================
// Audio capture
// ============================================================================

/// Audio source configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioConfig {
    /// Capture system audio (desktop audio output).
    pub system_audio: bool,
    /// Capture microphone input.
    pub microphone: bool,
    /// Microphone device name (if specific).
    pub mic_device: Option<String>,
    /// Audio sample rate.
    pub sample_rate: u32,
}

impl AudioConfig {
    pub fn default_config() -> Self {
        Self {
            system_audio: true,
            microphone: false,
            mic_device: None,
            sample_rate: 48000,
        }
    }

    pub fn none() -> Self {
        Self {
            system_audio: false,
            microphone: false,
            mic_device: None,
            sample_rate: 48000,
        }
    }

    pub fn has_audio(&self) -> bool {
        self.system_audio || self.microphone
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

// ============================================================================
// Output format
// ============================================================================

/// Recording output container format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    /// Raw frames (for compositor-level recording).
    RawFrames,
    /// AVI container.
    Avi,
    /// MP4 container (H.264 + AAC).
    Mp4,
    /// MKV container (flexible codec support).
    Mkv,
    /// WebM (VP9 + Opus).
    WebM,
}

impl OutputFormat {
    pub fn extension(&self) -> &str {
        match self {
            Self::RawFrames => "raw",
            Self::Avi => "avi",
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::WebM => "webm",
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::RawFrames => "Raw Frames",
            Self::Avi => "AVI",
            Self::Mp4 => "MP4 (H.264)",
            Self::Mkv => "MKV",
            Self::WebM => "WebM (VP9)",
        }
    }
}

// ============================================================================
// Recording state machine
// ============================================================================

/// Current state of the screen recorder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordingState {
    /// Idle, not recording.
    Idle,
    /// Countdown before recording starts.
    Countdown { remaining_secs: u32 },
    /// Selecting a region on screen.
    SelectingRegion,
    /// Actively recording.
    Recording,
    /// Paused (recording can be resumed).
    Paused,
    /// Encoding/processing after recording stopped.
    Processing,
}

impl RecordingState {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Recording | Self::Paused)
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Idle => "Ready",
            Self::Countdown { .. } => "Starting...",
            Self::SelectingRegion => "Select area",
            Self::Recording => "Recording",
            Self::Paused => "Paused",
            Self::Processing => "Processing",
        }
    }
}

// ============================================================================
// Recording configuration
// ============================================================================

/// Full recording configuration.
#[derive(Clone, Debug)]
pub struct RecordingConfig {
    pub capture_mode: CaptureMode,
    pub region: Option<CaptureRegion>,
    pub frame_rate: FrameRate,
    pub audio: AudioConfig,
    pub output_format: OutputFormat,
    /// Show cursor in recording.
    pub show_cursor: bool,
    /// Highlight cursor clicks.
    pub highlight_clicks: bool,
    /// Countdown seconds before recording (0 = immediate).
    pub countdown_secs: u32,
    /// Maximum recording duration in seconds (0 = unlimited).
    pub max_duration_secs: u32,
    /// Output file path.
    pub output_path: String,
    /// Show recording indicator overlay.
    pub show_indicator: bool,
}

impl RecordingConfig {
    pub fn default_config() -> Self {
        Self {
            capture_mode: CaptureMode::FullScreen,
            region: None,
            frame_rate: FrameRate::Fps30,
            audio: AudioConfig::default_config(),
            output_format: OutputFormat::Mp4,
            show_cursor: true,
            highlight_clicks: false,
            countdown_secs: 3,
            max_duration_secs: 0,
            output_path: "/home/recordings".to_string(),
            show_indicator: true,
        }
    }

    /// Get the effective capture region (based on mode).
    pub fn effective_region(&self) -> CaptureRegion {
        match self.capture_mode {
            CaptureMode::FullScreen => CaptureRegion::new(0, 0, 1920, 1080),
            CaptureMode::Monitor(_) => CaptureRegion::new(0, 0, 1920, 1080),
            CaptureMode::Window(_) => self.region.unwrap_or(CaptureRegion::new(0, 0, 800, 600)),
            CaptureMode::Region => self.region.unwrap_or(CaptureRegion::new(0, 0, 800, 600)),
        }
    }
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

// ============================================================================
// Recording session
// ============================================================================

/// Statistics for an active or completed recording.
#[derive(Clone, Debug)]
pub struct RecordingStats {
    /// Total frames captured.
    pub frames_captured: u64,
    /// Total elapsed recording time in milliseconds.
    pub elapsed_ms: u64,
    /// Dropped frames (couldn't keep up).
    pub dropped_frames: u64,
    /// Total bytes written.
    pub bytes_written: u64,
    /// Peak frame time in microseconds.
    pub peak_frame_time_us: u64,
}

impl RecordingStats {
    pub fn new() -> Self {
        Self {
            frames_captured: 0,
            elapsed_ms: 0,
            dropped_frames: 0,
            bytes_written: 0,
            peak_frame_time_us: 0,
        }
    }

    /// Elapsed time as HH:MM:SS string.
    pub fn elapsed_display(&self) -> String {
        let total_secs = self.elapsed_ms / 1000;
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        let secs = total_secs % 60;
        if hours > 0 {
            format!("{:02}:{:02}:{:02}", hours, mins, secs)
        } else {
            format!("{:02}:{:02}", mins, secs)
        }
    }

    /// Average fps achieved.
    pub fn average_fps(&self) -> f64 {
        if self.elapsed_ms == 0 {
            return 0.0;
        }
        (self.frames_captured as f64 * 1000.0) / self.elapsed_ms as f64
    }

    /// Drop rate as percentage.
    pub fn drop_rate_pct(&self) -> f64 {
        let total = self.frames_captured + self.dropped_frames;
        if total == 0 {
            return 0.0;
        }
        (self.dropped_frames as f64 / total as f64) * 100.0
    }

    /// Format bytes_written as human-readable.
    pub fn size_display(&self) -> String {
        format_bytes(self.bytes_written)
    }
}

impl Default for RecordingStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Format a byte count into human-readable form.
fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

// ============================================================================
// Recording history
// ============================================================================

/// A completed recording in history.
#[derive(Clone, Debug)]
pub struct RecordingEntry {
    pub id: u32,
    pub filename: String,
    pub timestamp: u64,
    pub duration_ms: u64,
    pub file_size: u64,
    pub region: CaptureRegion,
    pub frame_rate: FrameRate,
    pub format: OutputFormat,
}

impl RecordingEntry {
    pub fn duration_display(&self) -> String {
        let total_secs = self.duration_ms / 1000;
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        format!("{}:{:02}", mins, secs)
    }

    pub fn size_display(&self) -> String {
        format_bytes(self.file_size)
    }
}

// ============================================================================
// Screen Recorder Manager
// ============================================================================

/// Maximum recording history entries.
const MAX_HISTORY: usize = 50;

/// Manages screen recording sessions.
pub struct ScreenRecorder {
    pub config: RecordingConfig,
    pub state: RecordingState,
    pub stats: RecordingStats,
    pub history: Vec<RecordingEntry>,
    next_id: u32,
}

impl ScreenRecorder {
    pub fn new() -> Self {
        Self {
            config: RecordingConfig::default_config(),
            state: RecordingState::Idle,
            stats: RecordingStats::new(),
            history: Vec::new(),
            next_id: 1,
        }
    }

    /// Start a new recording (enters countdown or immediate start).
    pub fn start(&mut self) -> bool {
        if self.state != RecordingState::Idle {
            return false;
        }

        if self.config.capture_mode == CaptureMode::Region && self.config.region.is_none() {
            self.state = RecordingState::SelectingRegion;
            return true;
        }

        if self.config.countdown_secs > 0 {
            self.state = RecordingState::Countdown {
                remaining_secs: self.config.countdown_secs,
            };
        } else {
            self.state = RecordingState::Recording;
            self.stats = RecordingStats::new();
        }
        true
    }

    /// Advance countdown by one second. Returns true if recording started.
    pub fn tick_countdown(&mut self) -> bool {
        if let RecordingState::Countdown { remaining_secs } = self.state {
            if remaining_secs <= 1 {
                self.state = RecordingState::Recording;
                self.stats = RecordingStats::new();
                return true;
            }
            self.state = RecordingState::Countdown {
                remaining_secs: remaining_secs - 1,
            };
        }
        false
    }

    /// Confirm region selection and begin recording.
    pub fn confirm_region(&mut self, region: CaptureRegion) -> bool {
        if self.state != RecordingState::SelectingRegion {
            return false;
        }
        self.config.region = Some(region);
        if self.config.countdown_secs > 0 {
            self.state = RecordingState::Countdown {
                remaining_secs: self.config.countdown_secs,
            };
        } else {
            self.state = RecordingState::Recording;
            self.stats = RecordingStats::new();
        }
        true
    }

    /// Pause the recording.
    pub fn pause(&mut self) -> bool {
        if self.state == RecordingState::Recording {
            self.state = RecordingState::Paused;
            true
        } else {
            false
        }
    }

    /// Resume from pause.
    pub fn resume(&mut self) -> bool {
        if self.state == RecordingState::Paused {
            self.state = RecordingState::Recording;
            true
        } else {
            false
        }
    }

    /// Stop recording and save.
    pub fn stop(&mut self) -> bool {
        if !self.state.is_active() {
            return false;
        }
        self.state = RecordingState::Processing;
        true
    }

    /// Mark processing as complete and add to history.
    pub fn finish_processing(&mut self, filename: &str, file_size: u64, timestamp: u64) {
        let entry = RecordingEntry {
            id: self.next_id,
            filename: filename.to_string(),
            timestamp,
            duration_ms: self.stats.elapsed_ms,
            file_size,
            region: self.config.effective_region(),
            frame_rate: self.config.frame_rate,
            format: self.config.output_format,
        };
        self.next_id = self.next_id.saturating_add(1);

        if self.history.len() >= MAX_HISTORY {
            self.history.remove(0);
        }
        self.history.push(entry);

        self.state = RecordingState::Idle;
    }

    /// Cancel recording (discard).
    pub fn cancel(&mut self) {
        self.state = RecordingState::Idle;
        self.stats = RecordingStats::new();
    }

    /// Record a frame capture event.
    pub fn record_frame(&mut self, frame_time_us: u64) {
        if self.state != RecordingState::Recording {
            return;
        }
        self.stats.frames_captured = self.stats.frames_captured.saturating_add(1);
        if frame_time_us > self.stats.peak_frame_time_us {
            self.stats.peak_frame_time_us = frame_time_us;
        }
    }

    /// Record a dropped frame.
    pub fn record_dropped_frame(&mut self) {
        if self.state != RecordingState::Recording {
            return;
        }
        self.stats.dropped_frames = self.stats.dropped_frames.saturating_add(1);
    }

    /// Update elapsed time.
    pub fn update_elapsed(&mut self, elapsed_ms: u64) {
        self.stats.elapsed_ms = elapsed_ms;
    }

    /// Update bytes written.
    pub fn update_bytes_written(&mut self, bytes: u64) {
        self.stats.bytes_written = bytes;
    }

    /// Check if max duration reached.
    pub fn is_duration_exceeded(&self) -> bool {
        self.config.max_duration_secs > 0
            && self.stats.elapsed_ms >= self.config.max_duration_secs as u64 * 1000
    }

    /// Delete a history entry by ID.
    pub fn delete_history(&mut self, id: u32) -> bool {
        let before = self.history.len();
        self.history.retain(|e| e.id != id);
        self.history.len() < before
    }

    /// Clear all history.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}

impl Default for ScreenRecorder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Recording indicator overlay
// ============================================================================

/// Render the small recording indicator overlay (shown while recording).
pub fn render_recording_indicator(
    recorder: &ScreenRecorder,
    x: f32,
    y: f32,
) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();

    if !recorder.state.is_active() && !matches!(recorder.state, RecordingState::Processing) {
        return cmds;
    }

    let w = 160.0;
    let h = 32.0;

    // Background pill.
    cmds.push(RenderCommand::FillRect {
        x, y, width: w, height: h,
        color: Color::rgba(MOCHA_BASE.r, MOCHA_BASE.g, MOCHA_BASE.b, 220),
        corner_radii: CornerRadii::all(16.0),
    });

    // Recording dot (pulsing red when recording, yellow when paused).
    let dot_color = match recorder.state {
        RecordingState::Recording => MOCHA_RED,
        RecordingState::Paused => MOCHA_YELLOW,
        _ => MOCHA_OVERLAY0,
    };
    cmds.push(RenderCommand::FillRect {
        x: x + 10.0, y: y + 10.0, width: 12.0, height: 12.0,
        color: dot_color,
        corner_radii: CornerRadii::all(6.0),
    });

    // Time display.
    cmds.push(RenderCommand::Text {
        x: x + 28.0, y: y + 8.0,
        text: recorder.stats.elapsed_display(),
        font_size: 13.0, color: MOCHA_TEXT,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });

    // State label.
    let state_label = recorder.state.label();
    cmds.push(RenderCommand::Text {
        x: x + 90.0, y: y + 10.0,
        text: state_label.to_string(),
        font_size: 10.0, color: MOCHA_SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    cmds
}

/// Render the recording toolbar/control panel.
pub fn render_recording_controls(
    recorder: &ScreenRecorder,
    x: f32,
    y: f32,
    w: f32,
) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();
    let h = 60.0;

    // Background.
    cmds.push(RenderCommand::FillRect {
        x, y, width: w, height: h,
        color: MOCHA_MANTLE,
        corner_radii: CornerRadii::all(8.0),
    });

    // Title.
    cmds.push(RenderCommand::Text {
        x: x + 12.0, y: y + 6.0,
        text: "Screen Recorder".to_string(),
        font_size: 13.0, color: MOCHA_TEXT,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });

    let btn_y = y + 28.0;
    let btn_h = 24.0;

    match recorder.state {
        RecordingState::Idle => {
            // Record button.
            cmds.push(RenderCommand::FillRect {
                x: x + 12.0, y: btn_y, width: 80.0, height: btn_h,
                color: MOCHA_RED,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 26.0, y: btn_y + 5.0,
                text: "Record".to_string(),
                font_size: 12.0, color: MOCHA_BASE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
        RecordingState::Recording => {
            // Pause button.
            cmds.push(RenderCommand::FillRect {
                x: x + 12.0, y: btn_y, width: 70.0, height: btn_h,
                color: MOCHA_YELLOW,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 24.0, y: btn_y + 5.0,
                text: "Pause".to_string(),
                font_size: 12.0, color: MOCHA_BASE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Stop button.
            cmds.push(RenderCommand::FillRect {
                x: x + 92.0, y: btn_y, width: 60.0, height: btn_h,
                color: MOCHA_SURFACE1,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 106.0, y: btn_y + 5.0,
                text: "Stop".to_string(),
                font_size: 12.0, color: MOCHA_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
        RecordingState::Paused => {
            // Resume button.
            cmds.push(RenderCommand::FillRect {
                x: x + 12.0, y: btn_y, width: 80.0, height: btn_h,
                color: MOCHA_GREEN,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 22.0, y: btn_y + 5.0,
                text: "Resume".to_string(),
                font_size: 12.0, color: MOCHA_BASE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Stop button.
            cmds.push(RenderCommand::FillRect {
                x: x + 102.0, y: btn_y, width: 60.0, height: btn_h,
                color: MOCHA_SURFACE1,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 116.0, y: btn_y + 5.0,
                text: "Stop".to_string(),
                font_size: 12.0, color: MOCHA_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
        _ => {
            cmds.push(RenderCommand::Text {
                x: x + 12.0, y: btn_y + 4.0,
                text: recorder.state.label().to_string(),
                font_size: 12.0, color: MOCHA_PEACH,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    // Stats (right side).
    if recorder.state.is_active() || recorder.state == RecordingState::Processing {
        let stats_x = x + w - 200.0;
        cmds.push(RenderCommand::Text {
            x: stats_x, y: y + 8.0,
            text: format!("Frames: {} ({:.1} fps)",
                recorder.stats.frames_captured,
                recorder.stats.average_fps(),
            ),
            font_size: 10.0, color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: stats_x, y: y + 22.0,
            text: format!("Size: {} | Drops: {:.1}%",
                recorder.stats.size_display(),
                recorder.stats.drop_rate_pct(),
            ),
            font_size: 10.0, color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: stats_x, y: y + 36.0,
            text: format!("Time: {}", recorder.stats.elapsed_display()),
            font_size: 10.0, color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    cmds
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- CaptureRegion ---
    #[test]
    fn test_region_new() {
        let r = CaptureRegion::new(10, 20, 800, 600);
        assert_eq!(r.x, 10);
        assert_eq!(r.width, 800);
    }

    #[test]
    fn test_region_pixel_count() {
        let r = CaptureRegion::new(0, 0, 1920, 1080);
        assert_eq!(r.pixel_count(), 2_073_600);
    }

    #[test]
    fn test_region_frame_size() {
        let r = CaptureRegion::new(0, 0, 100, 100);
        assert_eq!(r.frame_size_bytes(), 40000);
    }

    #[test]
    fn test_region_valid() {
        assert!(CaptureRegion::new(0, 0, 100, 100).is_valid());
        assert!(!CaptureRegion::new(0, 0, 0, 100).is_valid());
        assert!(!CaptureRegion::new(0, 0, 100, 0).is_valid());
    }

    // --- FrameRate ---
    #[test]
    fn test_frame_rate_values() {
        assert_eq!(FrameRate::Fps15.value(), 15);
        assert_eq!(FrameRate::Fps30.value(), 30);
        assert_eq!(FrameRate::Fps60.value(), 60);
    }

    #[test]
    fn test_frame_rate_labels() {
        assert_eq!(FrameRate::Fps30.label(), "30 fps");
    }

    #[test]
    fn test_data_rate() {
        let region = CaptureRegion::new(0, 0, 1920, 1080);
        let rate = FrameRate::Fps30.data_rate_mbps(&region);
        assert!(rate > 200.0); // ~237 MB/s raw
    }

    // --- AudioConfig ---
    #[test]
    fn test_audio_default() {
        let a = AudioConfig::default_config();
        assert!(a.system_audio);
        assert!(!a.microphone);
        assert!(a.has_audio());
    }

    #[test]
    fn test_audio_none() {
        let a = AudioConfig::none();
        assert!(!a.has_audio());
    }

    // --- OutputFormat ---
    #[test]
    fn test_output_format_extensions() {
        assert_eq!(OutputFormat::Mp4.extension(), "mp4");
        assert_eq!(OutputFormat::WebM.extension(), "webm");
        assert_eq!(OutputFormat::Mkv.extension(), "mkv");
    }

    // --- RecordingState ---
    #[test]
    fn test_state_is_active() {
        assert!(!RecordingState::Idle.is_active());
        assert!(RecordingState::Recording.is_active());
        assert!(RecordingState::Paused.is_active());
        assert!(!RecordingState::Processing.is_active());
    }

    #[test]
    fn test_state_labels() {
        assert_eq!(RecordingState::Idle.label(), "Ready");
        assert_eq!(RecordingState::Recording.label(), "Recording");
    }

    // --- RecordingStats ---
    #[test]
    fn test_stats_elapsed_display() {
        let mut s = RecordingStats::new();
        s.elapsed_ms = 65000; // 1:05
        assert_eq!(s.elapsed_display(), "01:05");
        s.elapsed_ms = 3661000; // 1:01:01
        assert_eq!(s.elapsed_display(), "01:01:01");
    }

    #[test]
    fn test_stats_average_fps() {
        let mut s = RecordingStats::new();
        s.frames_captured = 300;
        s.elapsed_ms = 10000;
        assert!((s.average_fps() - 30.0).abs() < 0.01);
    }

    #[test]
    fn test_stats_average_fps_zero_time() {
        let s = RecordingStats::new();
        assert_eq!(s.average_fps(), 0.0);
    }

    #[test]
    fn test_stats_drop_rate() {
        let mut s = RecordingStats::new();
        s.frames_captured = 90;
        s.dropped_frames = 10;
        assert!((s.drop_rate_pct() - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_stats_drop_rate_zero() {
        let s = RecordingStats::new();
        assert_eq!(s.drop_rate_pct(), 0.0);
    }

    #[test]
    fn test_stats_size_display() {
        let mut s = RecordingStats::new();
        s.bytes_written = 500;
        assert_eq!(s.size_display(), "500 B");
        s.bytes_written = 2048;
        assert_eq!(s.size_display(), "2.0 KB");
        s.bytes_written = 5 * 1024 * 1024;
        assert_eq!(s.size_display(), "5.0 MB");
    }

    // --- RecordingEntry ---
    #[test]
    fn test_entry_duration_display() {
        let entry = RecordingEntry {
            id: 1, filename: "test.mp4".to_string(), timestamp: 0,
            duration_ms: 125000, file_size: 1024,
            region: CaptureRegion::new(0, 0, 800, 600),
            frame_rate: FrameRate::Fps30, format: OutputFormat::Mp4,
        };
        assert_eq!(entry.duration_display(), "2:05");
    }

    // --- ScreenRecorder ---
    #[test]
    fn test_recorder_new() {
        let r = ScreenRecorder::new();
        assert_eq!(r.state, RecordingState::Idle);
    }

    #[test]
    fn test_start_with_countdown() {
        let mut r = ScreenRecorder::new();
        assert!(r.start());
        assert!(matches!(r.state, RecordingState::Countdown { remaining_secs: 3 }));
    }

    #[test]
    fn test_start_immediate() {
        let mut r = ScreenRecorder::new();
        r.config.countdown_secs = 0;
        assert!(r.start());
        assert_eq!(r.state, RecordingState::Recording);
    }

    #[test]
    fn test_start_while_recording_fails() {
        let mut r = ScreenRecorder::new();
        r.config.countdown_secs = 0;
        r.start();
        assert!(!r.start());
    }

    #[test]
    fn test_countdown_tick() {
        let mut r = ScreenRecorder::new();
        r.start(); // countdown = 3
        assert!(!r.tick_countdown()); // 3 → 2
        assert!(!r.tick_countdown()); // 2 → 1
        assert!(r.tick_countdown()); // 1 → recording
        assert_eq!(r.state, RecordingState::Recording);
    }

    #[test]
    fn test_region_selection_flow() {
        let mut r = ScreenRecorder::new();
        r.config.capture_mode = CaptureMode::Region;
        r.config.countdown_secs = 0;
        r.start();
        assert_eq!(r.state, RecordingState::SelectingRegion);

        r.confirm_region(CaptureRegion::new(10, 20, 400, 300));
        assert_eq!(r.state, RecordingState::Recording);
        assert_eq!(r.config.region.unwrap().width, 400);
    }

    #[test]
    fn test_pause_resume() {
        let mut r = ScreenRecorder::new();
        r.config.countdown_secs = 0;
        r.start();
        assert!(r.pause());
        assert_eq!(r.state, RecordingState::Paused);
        assert!(r.resume());
        assert_eq!(r.state, RecordingState::Recording);
    }

    #[test]
    fn test_pause_when_not_recording() {
        let mut r = ScreenRecorder::new();
        assert!(!r.pause());
    }

    #[test]
    fn test_stop() {
        let mut r = ScreenRecorder::new();
        r.config.countdown_secs = 0;
        r.start();
        assert!(r.stop());
        assert_eq!(r.state, RecordingState::Processing);
    }

    #[test]
    fn test_stop_when_idle_fails() {
        let mut r = ScreenRecorder::new();
        assert!(!r.stop());
    }

    #[test]
    fn test_finish_processing() {
        let mut r = ScreenRecorder::new();
        r.config.countdown_secs = 0;
        r.start();
        r.stats.elapsed_ms = 5000;
        r.stop();
        r.finish_processing("video.mp4", 1024000, 999);
        assert_eq!(r.state, RecordingState::Idle);
        assert_eq!(r.history.len(), 1);
        assert_eq!(r.history[0].filename, "video.mp4");
    }

    #[test]
    fn test_cancel() {
        let mut r = ScreenRecorder::new();
        r.config.countdown_secs = 0;
        r.start();
        r.record_frame(100);
        r.cancel();
        assert_eq!(r.state, RecordingState::Idle);
        assert_eq!(r.stats.frames_captured, 0); // Stats reset
    }

    #[test]
    fn test_record_frame() {
        let mut r = ScreenRecorder::new();
        r.config.countdown_secs = 0;
        r.start();
        r.record_frame(500);
        r.record_frame(800);
        assert_eq!(r.stats.frames_captured, 2);
        assert_eq!(r.stats.peak_frame_time_us, 800);
    }

    #[test]
    fn test_record_frame_not_recording() {
        let mut r = ScreenRecorder::new();
        r.record_frame(500);
        assert_eq!(r.stats.frames_captured, 0);
    }

    #[test]
    fn test_dropped_frames() {
        let mut r = ScreenRecorder::new();
        r.config.countdown_secs = 0;
        r.start();
        r.record_dropped_frame();
        r.record_dropped_frame();
        assert_eq!(r.stats.dropped_frames, 2);
    }

    #[test]
    fn test_duration_exceeded() {
        let mut r = ScreenRecorder::new();
        r.config.max_duration_secs = 60;
        r.update_elapsed(59000);
        assert!(!r.is_duration_exceeded());
        r.update_elapsed(60000);
        assert!(r.is_duration_exceeded());
    }

    #[test]
    fn test_duration_unlimited() {
        let mut r = ScreenRecorder::new();
        r.config.max_duration_secs = 0;
        r.update_elapsed(999999);
        assert!(!r.is_duration_exceeded());
    }

    #[test]
    fn test_history_max() {
        let mut r = ScreenRecorder::new();
        for i in 0..MAX_HISTORY + 5 {
            r.config.countdown_secs = 0;
            r.start();
            r.stop();
            r.finish_processing(&format!("v{}.mp4", i), 100, i as u64);
        }
        assert_eq!(r.history.len(), MAX_HISTORY);
    }

    #[test]
    fn test_delete_history() {
        let mut r = ScreenRecorder::new();
        r.config.countdown_secs = 0;
        r.start();
        r.stop();
        r.finish_processing("test.mp4", 100, 0);
        let id = r.history[0].id;
        assert!(r.delete_history(id));
        assert!(r.history.is_empty());
    }

    #[test]
    fn test_clear_history() {
        let mut r = ScreenRecorder::new();
        r.config.countdown_secs = 0;
        r.start(); r.stop(); r.finish_processing("a.mp4", 1, 0);
        r.start(); r.stop(); r.finish_processing("b.mp4", 2, 1);
        assert_eq!(r.history.len(), 2);
        r.clear_history();
        assert!(r.history.is_empty());
    }

    // --- Rendering ---
    #[test]
    fn test_indicator_idle_empty() {
        let r = ScreenRecorder::new();
        let cmds = render_recording_indicator(&r, 0.0, 0.0);
        assert!(cmds.is_empty()); // No indicator when idle
    }

    #[test]
    fn test_indicator_recording() {
        let mut r = ScreenRecorder::new();
        r.config.countdown_secs = 0;
        r.start();
        let cmds = render_recording_indicator(&r, 10.0, 10.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_controls_idle() {
        let r = ScreenRecorder::new();
        let cmds = render_recording_controls(&r, 0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_controls_recording() {
        let mut r = ScreenRecorder::new();
        r.config.countdown_secs = 0;
        r.start();
        let cmds = render_recording_controls(&r, 0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_controls_paused() {
        let mut r = ScreenRecorder::new();
        r.config.countdown_secs = 0;
        r.start();
        r.pause();
        let cmds = render_recording_controls(&r, 0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    // --- Format helpers ---
    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1048576), "1.0 MB");
        assert_eq!(format_bytes(1073741824), "1.00 GB");
    }

    // --- Config ---
    #[test]
    fn test_config_defaults() {
        let cfg = RecordingConfig::default_config();
        assert_eq!(cfg.capture_mode, CaptureMode::FullScreen);
        assert_eq!(cfg.frame_rate, FrameRate::Fps30);
        assert!(cfg.show_cursor);
        assert_eq!(cfg.countdown_secs, 3);
    }

    #[test]
    fn test_effective_region_fullscreen() {
        let cfg = RecordingConfig::default_config();
        let r = cfg.effective_region();
        assert_eq!(r.width, 1920);
        assert_eq!(r.height, 1080);
    }

    #[test]
    fn test_capture_mode_labels() {
        assert_eq!(CaptureMode::FullScreen.label(), "Full Screen");
        assert_eq!(CaptureMode::Region.label(), "Region");
    }

    #[test]
    fn test_default_trait_impls() {
        let _ = AudioConfig::default();
        let _ = RecordingConfig::default();
        let _ = RecordingStats::default();
        let _ = ScreenRecorder::default();
    }
}
