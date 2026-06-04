//! OurOS Screen Recorder
//!
//! A screen recording/capture application providing:
//! - Capture modes: full screen, selected window, custom region (drag to select)
//! - Recording formats: raw BMP frame sequences at configurable FPS (15/24/30/60)
//! - Audio capture: system audio toggle, microphone toggle, volume levels
//! - Countdown timer: 3-2-1 countdown before recording starts
//! - Recording controls: start/stop/pause/resume with keyboard shortcuts
//! - Floating recording indicator: elapsed time, file size, FPS
//! - Mouse cursor: capture/hide option, click highlight
//! - Annotation tools: rectangle, arrow, text, highlight drawn during recording
//! - Output settings: save location, filename template, auto-increment, max file size
//! - Recording history: previous recordings with thumbnails, duration, file size
//! - Trimming: basic start/end trim for recorded clips
//! - Hotkeys: configurable global hotkeys for start/stop/pause/screenshot
//! - Scheduled recording: timed recordings (start at time, record for duration)
//! - Catppuccin Mocha dark theme throughout
//!
//! Uses the guitk library for UI rendering.

#![allow(dead_code, clippy::too_many_arguments)]

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::path::PathBuf;

// ============================================================================
// Catppuccin Mocha color palette
// ============================================================================

mod colors {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const MANTLE: Color = Color::from_hex(0x181825);
    pub const CRUST: Color = Color::from_hex(0x11111B);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const SURFACE2: Color = Color::from_hex(0x585B70);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const LAVENDER: Color = Color::from_hex(0xB4BEFE);
}

// ============================================================================
// UI layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 900.0;
const WINDOW_HEIGHT: f32 = 640.0;
const SIDEBAR_WIDTH: f32 = 200.0;
const TOOLBAR_HEIGHT: f32 = 48.0;
const STATUS_BAR_HEIGHT: f32 = 30.0;
const BUTTON_HEIGHT: f32 = 34.0;
const BUTTON_SPACING: f32 = 8.0;
const PADDING: f32 = 12.0;
const SECTION_SPACING: f32 = 16.0;
const ICON_SIZE: f32 = 16.0;
const CORNER_RADIUS: f32 = 6.0;
const SMALL_RADIUS: f32 = 4.0;

/// Default save directory path.
const DEFAULT_SAVE_DIR: &str = "~/Videos/Recordings/";

/// Default filename template. `{n}` is replaced with the auto-increment number,
/// `{date}` with YYYYMMDD, `{time}` with HHMMSS.
const DEFAULT_FILENAME_TEMPLATE: &str = "recording_{date}_{time}";

/// BMP file header size (BITMAPFILEHEADER).
const BMP_FILE_HEADER_SIZE: u32 = 14;

/// BMP info header size (BITMAPINFOHEADER).
const BMP_INFO_HEADER_SIZE: u32 = 40;

/// Bytes per pixel in 32-bit BMP output.
const BMP_BYTES_PER_PIXEL: u32 = 4;

// ============================================================================
// Capture mode
// ============================================================================

/// The type of screen capture region for recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureMode {
    /// Record the entire screen.
    FullScreen,
    /// Record a specific window selected by the user.
    SelectedWindow,
    /// Record a custom rectangular region defined by dragging.
    CustomRegion,
}

impl CaptureMode {
    /// Human-readable label for UI display.
    pub fn label(self) -> &'static str {
        match self {
            Self::FullScreen => "Full Screen",
            Self::SelectedWindow => "Selected Window",
            Self::CustomRegion => "Custom Region",
        }
    }

    /// All available capture modes.
    pub fn all() -> &'static [CaptureMode] {
        &[Self::FullScreen, Self::SelectedWindow, Self::CustomRegion]
    }
}

// ============================================================================
// FPS presets
// ============================================================================

/// Supported recording frame rates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FpsPreset {
    Fps15,
    Fps24,
    Fps30,
    Fps60,
}

impl FpsPreset {
    /// Numeric frames per second value.
    pub fn value(self) -> u32 {
        match self {
            Self::Fps15 => 15,
            Self::Fps24 => 24,
            Self::Fps30 => 30,
            Self::Fps60 => 60,
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Fps15 => "15 FPS",
            Self::Fps24 => "24 FPS",
            Self::Fps30 => "30 FPS",
            Self::Fps60 => "60 FPS",
        }
    }

    /// All available FPS presets.
    pub fn all() -> &'static [FpsPreset] {
        &[Self::Fps15, Self::Fps24, Self::Fps30, Self::Fps60]
    }

    /// Milliseconds between frames at this rate.
    pub fn frame_interval_ms(self) -> u32 {
        1000 / self.value()
    }

    /// Estimated bytes per frame for a given resolution (32-bit BGRA).
    pub fn bytes_per_frame(self, width: u32, height: u32) -> u64 {
        let _ = self; // FPS doesn't affect per-frame size
        u64::from(width) * u64::from(height) * u64::from(BMP_BYTES_PER_PIXEL)
    }

    /// Estimated bytes per second for a given resolution (uncompressed BMP).
    pub fn bytes_per_second(self, width: u32, height: u32) -> u64 {
        self.bytes_per_frame(width, height) * u64::from(self.value())
    }
}

// ============================================================================
// Recording state machine
// ============================================================================

/// Recording lifecycle states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordingState {
    /// Idle, ready to start a new recording.
    Idle,
    /// Countdown timer is running before recording begins.
    Countdown,
    /// Actively recording frames.
    Recording,
    /// Recording is paused; can resume or stop.
    Paused,
    /// Recording completed; available for review/trim/save.
    Stopped,
}

impl RecordingState {
    /// Returns the valid transitions from the current state.
    pub fn allowed_transitions(self) -> &'static [RecordingState] {
        match self {
            Self::Idle => &[Self::Countdown, Self::Recording],
            Self::Countdown => &[Self::Idle], // cancel countdown
            Self::Recording => &[Self::Paused, Self::Stopped],
            Self::Paused => &[Self::Recording, Self::Stopped],
            Self::Stopped => &[Self::Idle],
        }
    }

    /// Whether transitioning to the given target state is valid.
    pub fn can_transition_to(self, target: RecordingState) -> bool {
        self.allowed_transitions().contains(&target)
    }

    /// Human-readable label for this state.
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Ready",
            Self::Countdown => "Countdown",
            Self::Recording => "Recording",
            Self::Paused => "Paused",
            Self::Stopped => "Stopped",
        }
    }

    /// Color associated with this state for UI display.
    pub fn color(self) -> Color {
        match self {
            Self::Idle => colors::SUBTEXT0,
            Self::Countdown => colors::YELLOW,
            Self::Recording => colors::RED,
            Self::Paused => colors::PEACH,
            Self::Stopped => colors::GREEN,
        }
    }

    /// Whether the recording is in an active/in-progress state.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Countdown | Self::Recording | Self::Paused)
    }
}

// ============================================================================
// Audio capture settings
// ============================================================================

/// Audio capture configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioSettings {
    /// Whether to capture system/desktop audio.
    pub system_audio_enabled: bool,
    /// Whether to capture microphone input.
    pub microphone_enabled: bool,
    /// System audio volume level (0.0 to 1.0).
    pub system_volume: f32,
    /// Microphone volume level (0.0 to 1.0).
    pub microphone_volume: f32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            system_audio_enabled: true,
            microphone_enabled: false,
            system_volume: 0.8,
            microphone_volume: 0.7,
        }
    }
}

impl AudioSettings {
    /// Create new audio settings with the given toggles.
    pub fn new(system_audio: bool, microphone: bool) -> Self {
        Self {
            system_audio_enabled: system_audio,
            microphone_enabled: microphone,
            ..Default::default()
        }
    }

    /// Set system audio volume, clamped to [0.0, 1.0].
    pub fn set_system_volume(&mut self, volume: f32) {
        self.system_volume = volume.clamp(0.0, 1.0);
    }

    /// Set microphone volume, clamped to [0.0, 1.0].
    pub fn set_microphone_volume(&mut self, volume: f32) {
        self.microphone_volume = volume.clamp(0.0, 1.0);
    }

    /// Whether any audio source is enabled.
    pub fn any_audio_enabled(&self) -> bool {
        self.system_audio_enabled || self.microphone_enabled
    }

    /// System volume as a percentage integer (0-100).
    pub fn system_volume_percent(&self) -> u32 {
        (self.system_volume * 100.0) as u32
    }

    /// Microphone volume as a percentage integer (0-100).
    pub fn microphone_volume_percent(&self) -> u32 {
        (self.microphone_volume * 100.0) as u32
    }
}

// ============================================================================
// Countdown timer
// ============================================================================

/// Countdown timer state for the pre-recording delay.
#[derive(Clone, Debug, PartialEq)]
pub struct CountdownTimer {
    /// Total countdown duration in seconds.
    pub duration_secs: u32,
    /// Remaining seconds. 0 means the countdown has finished.
    pub remaining_secs: u32,
    /// Whether the countdown is currently active.
    pub active: bool,
}

impl CountdownTimer {
    /// Create a new countdown timer with the given duration.
    pub fn new(duration_secs: u32) -> Self {
        Self {
            duration_secs,
            remaining_secs: duration_secs,
            active: false,
        }
    }

    /// Start the countdown.
    pub fn start(&mut self) {
        self.remaining_secs = self.duration_secs;
        self.active = true;
    }

    /// Advance the countdown by one second. Returns true if the countdown
    /// has reached zero (recording should begin).
    pub fn tick(&mut self) -> bool {
        if !self.active || self.remaining_secs == 0 {
            return false;
        }
        self.remaining_secs = self.remaining_secs.saturating_sub(1);
        if self.remaining_secs == 0 {
            self.active = false;
            return true;
        }
        false
    }

    /// Cancel an in-progress countdown.
    pub fn cancel(&mut self) {
        self.active = false;
        self.remaining_secs = self.duration_secs;
    }

    /// Whether the countdown has finished (reached zero).
    pub fn is_finished(&self) -> bool {
        !self.active && self.remaining_secs == 0
    }

    /// Reset to initial state.
    pub fn reset(&mut self) {
        self.remaining_secs = self.duration_secs;
        self.active = false;
    }

    /// Set a new countdown duration.
    pub fn set_duration(&mut self, secs: u32) {
        self.duration_secs = secs;
        if !self.active {
            self.remaining_secs = secs;
        }
    }
}

// ============================================================================
// Mouse cursor options
// ============================================================================

/// Mouse cursor capture options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorOptions {
    /// Whether to include the mouse cursor in the recording.
    pub capture_cursor: bool,
    /// Whether to show a visual highlight on mouse clicks.
    pub click_highlight: bool,
    /// Highlight circle radius in pixels.
    pub highlight_radius: u32,
}

impl Default for CursorOptions {
    fn default() -> Self {
        Self {
            capture_cursor: true,
            click_highlight: true,
            highlight_radius: 20,
        }
    }
}

impl CursorOptions {
    /// Create new cursor options.
    pub fn new(capture: bool, highlight: bool) -> Self {
        Self {
            capture_cursor: capture,
            click_highlight: highlight,
            ..Default::default()
        }
    }
}

// ============================================================================
// Annotation tools
// ============================================================================

/// Available annotation drawing tools for live recording overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationTool {
    /// Draw a colored rectangle outline.
    Rectangle,
    /// Draw an arrow from start to end point.
    Arrow,
    /// Place a text label at a position.
    Text,
    /// Draw a semi-transparent highlight region.
    Highlight,
}

impl AnnotationTool {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Rectangle => "Rectangle",
            Self::Arrow => "Arrow",
            Self::Text => "Text",
            Self::Highlight => "Highlight",
        }
    }

    /// All available annotation tools.
    pub fn all() -> &'static [AnnotationTool] {
        &[Self::Rectangle, Self::Arrow, Self::Text, Self::Highlight]
    }

    /// Color used by default for this tool type.
    pub fn default_color(self) -> Color {
        match self {
            Self::Rectangle => colors::RED,
            Self::Arrow => colors::BLUE,
            Self::Text => colors::TEXT,
            Self::Highlight => Color::rgba(249, 226, 175, 80), // semi-transparent yellow
        }
    }
}

/// A single annotation drawn over the recording.
#[derive(Clone, Debug)]
pub struct Annotation {
    pub tool: AnnotationTool,
    pub start_x: f32,
    pub start_y: f32,
    pub end_x: f32,
    pub end_y: f32,
    pub color: Color,
    pub text_content: String,
}

impl Annotation {
    /// Create a new annotation at the given start position.
    pub fn new(tool: AnnotationTool, start_x: f32, start_y: f32, color: Color) -> Self {
        Self {
            tool,
            start_x,
            start_y,
            end_x: start_x,
            end_y: start_y,
            color,
            text_content: String::new(),
        }
    }

    /// Width of the annotation bounding box.
    pub fn width(&self) -> f32 {
        (self.end_x - self.start_x).abs()
    }

    /// Height of the annotation bounding box.
    pub fn height(&self) -> f32 {
        (self.end_y - self.start_y).abs()
    }

    /// Top-left X of the bounding box.
    pub fn min_x(&self) -> f32 {
        self.start_x.min(self.end_x)
    }

    /// Top-left Y of the bounding box.
    pub fn min_y(&self) -> f32 {
        self.start_y.min(self.end_y)
    }

    /// Render this annotation as render commands.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        match self.tool {
            AnnotationTool::Rectangle => {
                cmds.push(RenderCommand::StrokeRect {
                    x: self.min_x(),
                    y: self.min_y(),
                    width: self.width(),
                    height: self.height(),
                    color: self.color,
                    line_width: 2.0,
                    corner_radii: CornerRadii::ZERO,
                });
            }
            AnnotationTool::Arrow => {
                cmds.push(RenderCommand::Line {
                    x1: self.start_x,
                    y1: self.start_y,
                    x2: self.end_x,
                    y2: self.end_y,
                    color: self.color,
                    width: 2.0,
                });
                // Arrowhead: two short lines from the end point
                let dx = self.end_x - self.start_x;
                let dy = self.end_y - self.start_y;
                let len = (dx * dx + dy * dy).sqrt();
                if len > 1.0 {
                    let ux = dx / len;
                    let uy = dy / len;
                    let head_len: f32 = 12.0;
                    let head_spread: f32 = 6.0;
                    // Left barb
                    cmds.push(RenderCommand::Line {
                        x1: self.end_x,
                        y1: self.end_y,
                        x2: self.end_x - ux * head_len + uy * head_spread,
                        y2: self.end_y - uy * head_len - ux * head_spread,
                        color: self.color,
                        width: 2.0,
                    });
                    // Right barb
                    cmds.push(RenderCommand::Line {
                        x1: self.end_x,
                        y1: self.end_y,
                        x2: self.end_x - ux * head_len - uy * head_spread,
                        y2: self.end_y - uy * head_len + ux * head_spread,
                        color: self.color,
                        width: 2.0,
                    });
                }
            }
            AnnotationTool::Text => {
                cmds.push(RenderCommand::Text {
                    x: self.start_x,
                    y: self.start_y,
                    text: if self.text_content.is_empty() {
                        "Text".to_string()
                    } else {
                        self.text_content.clone()
                    },
                    font_size: 16.0,
                    color: self.color,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }
            AnnotationTool::Highlight => {
                cmds.push(RenderCommand::FillRect {
                    x: self.min_x(),
                    y: self.min_y(),
                    width: self.width(),
                    height: self.height(),
                    color: self.color,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }
        cmds
    }
}

// ============================================================================
// Output settings
// ============================================================================

/// Output/save configuration for recordings.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputSettings {
    /// Directory to save recordings in.
    pub save_directory: PathBuf,
    /// Filename template. Supports `{date}`, `{time}`, `{n}` placeholders.
    pub filename_template: String,
    /// Auto-increment counter for `{n}` placeholder.
    pub auto_increment: u32,
    /// Maximum file size limit in bytes (0 = no limit).
    pub max_file_size: u64,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            save_directory: PathBuf::from(DEFAULT_SAVE_DIR),
            filename_template: DEFAULT_FILENAME_TEMPLATE.to_string(),
            auto_increment: 1,
            max_file_size: 0,
        }
    }
}

impl OutputSettings {
    /// Generate an output filename from the template and a timestamp.
    pub fn generate_filename(
        &self,
        year: u16,
        month: u8,
        day: u8,
        hour: u8,
        min: u8,
        sec: u8,
    ) -> String {
        let date_str = format!("{:04}{:02}{:02}", year, month, day);
        let time_str = format!("{:02}{:02}{:02}", hour, min, sec);

        

        self
            .filename_template
            .replace("{date}", &date_str)
            .replace("{time}", &time_str)
            .replace("{n}", &format!("{:04}", self.auto_increment))
    }

    /// Generate the full output path including directory and extension.
    pub fn generate_path(
        &self,
        year: u16,
        month: u8,
        day: u8,
        hour: u8,
        min: u8,
        sec: u8,
    ) -> PathBuf {
        let filename = self.generate_filename(year, month, day, hour, min, sec);
        self.save_directory.join(filename)
    }

    /// Increment the auto-increment counter after a recording is saved.
    pub fn bump_increment(&mut self) {
        self.auto_increment = self.auto_increment.saturating_add(1);
    }

    /// Whether a file size limit is configured.
    pub fn has_size_limit(&self) -> bool {
        self.max_file_size > 0
    }

    /// Check if a given size (bytes) exceeds the limit. Always false if no limit.
    pub fn exceeds_limit(&self, size: u64) -> bool {
        self.has_size_limit() && size >= self.max_file_size
    }

    /// Human-readable string for the max file size.
    pub fn max_size_display(&self) -> String {
        if self.max_file_size == 0 {
            return "No limit".to_string();
        }
        format_file_size(self.max_file_size)
    }
}

/// Format a byte count as a human-readable file size.
pub fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Format a duration in seconds as HH:MM:SS.
pub fn format_duration(total_secs: u64) -> String {
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, secs)
    } else {
        format!("{:02}:{:02}", minutes, secs)
    }
}

// ============================================================================
// Recording history
// ============================================================================

/// A single entry in the recording history.
#[derive(Clone, Debug)]
pub struct HistoryEntry {
    /// Unique identifier for this recording.
    pub id: u32,
    /// Display name / filename.
    pub name: String,
    /// Full path to the recording directory or file.
    pub path: PathBuf,
    /// Recording duration in seconds.
    pub duration_secs: u64,
    /// Total file size in bytes.
    pub file_size: u64,
    /// Frame count.
    pub frame_count: u32,
    /// FPS used during recording.
    pub fps: u32,
    /// Capture resolution.
    pub width: u32,
    pub height: u32,
    /// Timestamp: (year, month, day, hour, minute, second).
    pub timestamp: (u16, u8, u8, u8, u8, u8),
    /// Whether this entry is currently selected in the UI.
    pub selected: bool,
}

impl HistoryEntry {
    /// Create a new history entry.
    pub fn new(
        id: u32,
        name: String,
        path: PathBuf,
        duration_secs: u64,
        file_size: u64,
        frame_count: u32,
        fps: u32,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            id,
            name,
            path,
            duration_secs,
            file_size,
            frame_count,
            fps,
            width,
            height,
            timestamp: (2026, 1, 1, 0, 0, 0),
            selected: false,
        }
    }

    /// Set the timestamp.
    pub fn with_timestamp(
        mut self,
        year: u16,
        month: u8,
        day: u8,
        hour: u8,
        min: u8,
        sec: u8,
    ) -> Self {
        self.timestamp = (year, month, day, hour, min, sec);
        self
    }

    /// Human-readable duration.
    pub fn duration_display(&self) -> String {
        format_duration(self.duration_secs)
    }

    /// Human-readable file size.
    pub fn size_display(&self) -> String {
        format_file_size(self.file_size)
    }

    /// Resolution display string.
    pub fn resolution_display(&self) -> String {
        format!("{}x{}", self.width, self.height)
    }

    /// Formatted timestamp string.
    pub fn timestamp_display(&self) -> String {
        let (y, mo, d, h, mi, s) = self.timestamp;
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            y, mo, d, h, mi, s
        )
    }
}

/// Recording history manager.
#[derive(Clone, Debug, Default)]
pub struct RecordingHistory {
    pub entries: Vec<HistoryEntry>,
    next_id: u32,
}

impl RecordingHistory {
    /// Create an empty history.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
        }
    }

    /// Add a new entry to the history, returning its ID.
    pub fn add(&mut self, entry: HistoryEntry) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let mut entry = entry;
        entry.id = id;
        self.entries.push(entry);
        id
    }

    /// Remove an entry by ID.
    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() != before
    }

    /// Get an entry by ID.
    pub fn get(&self, id: u32) -> Option<&HistoryEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Total number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the history is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clear all history entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Total file size across all entries.
    pub fn total_size(&self) -> u64 {
        self.entries.iter().map(|e| e.file_size).sum()
    }

    /// Select an entry by ID, deselecting all others.
    pub fn select(&mut self, id: u32) {
        for entry in &mut self.entries {
            entry.selected = entry.id == id;
        }
    }

    /// Get the currently selected entry.
    pub fn selected(&self) -> Option<&HistoryEntry> {
        self.entries.iter().find(|e| e.selected)
    }

    /// Create mock history entries for UI development.
    pub fn mock_entries() -> Self {
        let mut history = Self::new();
        history.add(
            HistoryEntry::new(
                0,
                "Desktop Recording".to_string(),
                PathBuf::from("~/Videos/Recordings/recording_20260518_120000"),
                125,
                1_048_576_000,
                3750,
                30,
                1920,
                1080,
            )
            .with_timestamp(2026, 5, 18, 12, 0, 0),
        );
        history.add(
            HistoryEntry::new(
                0,
                "App Demo".to_string(),
                PathBuf::from("~/Videos/Recordings/recording_20260517_143000"),
                45,
                376_832_000,
                1350,
                30,
                1920,
                1080,
            )
            .with_timestamp(2026, 5, 17, 14, 30, 0),
        );
        history.add(
            HistoryEntry::new(
                0,
                "Bug Report".to_string(),
                PathBuf::from("~/Videos/Recordings/recording_20260516_093000"),
                18,
                75_366_400,
                270,
                15,
                1280,
                720,
            )
            .with_timestamp(2026, 5, 16, 9, 30, 0),
        );
        history
    }
}

// ============================================================================
// Trim tool
// ============================================================================

/// Trim settings for a recorded clip.
#[derive(Clone, Debug, PartialEq)]
pub struct TrimRange {
    /// Start time in seconds (inclusive).
    pub start_secs: f64,
    /// End time in seconds (inclusive).
    pub end_secs: f64,
    /// Total clip duration in seconds.
    pub total_duration: f64,
}

impl TrimRange {
    /// Create a new trim range spanning the full duration.
    pub fn new(total_duration: f64) -> Self {
        Self {
            start_secs: 0.0,
            end_secs: total_duration,
            total_duration,
        }
    }

    /// Set the start time, clamped to [0, end_secs).
    pub fn set_start(&mut self, secs: f64) {
        self.start_secs = secs.clamp(0.0, self.end_secs - 0.1);
    }

    /// Set the end time, clamped to (start_secs, total_duration].
    pub fn set_end(&mut self, secs: f64) {
        self.end_secs = secs.clamp(self.start_secs + 0.1, self.total_duration);
    }

    /// Duration of the trimmed clip.
    pub fn trimmed_duration(&self) -> f64 {
        self.end_secs - self.start_secs
    }

    /// Whether the clip has been trimmed (i.e., not full duration).
    pub fn is_trimmed(&self) -> bool {
        self.start_secs > 0.001 || (self.total_duration - self.end_secs) > 0.001
    }

    /// Start fraction (0.0 to 1.0).
    pub fn start_fraction(&self) -> f64 {
        if self.total_duration <= 0.0 {
            return 0.0;
        }
        self.start_secs / self.total_duration
    }

    /// End fraction (0.0 to 1.0).
    pub fn end_fraction(&self) -> f64 {
        if self.total_duration <= 0.0 {
            return 1.0;
        }
        self.end_secs / self.total_duration
    }

    /// Number of frames in the trimmed range at the given FPS.
    pub fn trimmed_frame_count(&self, fps: u32) -> u32 {
        (self.trimmed_duration() * f64::from(fps)).round() as u32
    }

    /// Reset to full duration.
    pub fn reset(&mut self) {
        self.start_secs = 0.0;
        self.end_secs = self.total_duration;
    }
}

// ============================================================================
// Hotkey configuration
// ============================================================================

/// A keyboard shortcut binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HotkeyBinding {
    /// The action this hotkey triggers.
    pub action: HotkeyAction,
    /// Primary key name (e.g., "F9", "PrintScreen").
    pub key: String,
    /// Whether Ctrl modifier is required.
    pub ctrl: bool,
    /// Whether Shift modifier is required.
    pub shift: bool,
    /// Whether Alt modifier is required.
    pub alt: bool,
}

impl HotkeyBinding {
    /// Create a new hotkey binding.
    pub fn new(action: HotkeyAction, key: &str, ctrl: bool, shift: bool, alt: bool) -> Self {
        Self {
            action,
            key: key.to_string(),
            ctrl,
            shift,
            alt,
        }
    }

    /// Human-readable display string for this binding.
    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.alt {
            parts.push("Alt");
        }
        parts.push(&self.key);
        parts.join("+")
    }
}

/// Actions that can be triggered by hotkeys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyAction {
    /// Start or stop a recording.
    StartStop,
    /// Pause or resume a recording.
    PauseResume,
    /// Take a screenshot during recording.
    Screenshot,
    /// Cancel the current operation.
    Cancel,
}

impl HotkeyAction {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::StartStop => "Start / Stop",
            Self::PauseResume => "Pause / Resume",
            Self::Screenshot => "Screenshot",
            Self::Cancel => "Cancel",
        }
    }

    /// All available actions.
    pub fn all() -> &'static [HotkeyAction] {
        &[Self::StartStop, Self::PauseResume, Self::Screenshot, Self::Cancel]
    }
}

/// Default hotkey configuration.
pub fn default_hotkeys() -> Vec<HotkeyBinding> {
    vec![
        HotkeyBinding::new(HotkeyAction::StartStop, "F9", false, false, false),
        HotkeyBinding::new(HotkeyAction::PauseResume, "F10", false, false, false),
        HotkeyBinding::new(HotkeyAction::Screenshot, "PrintScreen", false, false, false),
        HotkeyBinding::new(HotkeyAction::Cancel, "Escape", false, false, false),
    ]
}

// ============================================================================
// Scheduled recording
// ============================================================================

/// A scheduled recording configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduledRecording {
    /// Unique identifier.
    pub id: u32,
    /// When to start recording: (hour, minute) in 24h format.
    pub start_hour: u8,
    pub start_minute: u8,
    /// Recording duration in seconds (0 = indefinite until manual stop).
    pub duration_secs: u64,
    /// Whether this schedule is enabled.
    pub enabled: bool,
    /// Optional label / description.
    pub label: String,
    /// Whether this is a one-shot schedule (runs once then disables itself).
    pub one_shot: bool,
}

impl ScheduledRecording {
    /// Create a new scheduled recording.
    pub fn new(
        id: u32,
        start_hour: u8,
        start_minute: u8,
        duration_secs: u64,
        label: &str,
    ) -> Self {
        Self {
            id,
            start_hour: start_hour.min(23),
            start_minute: start_minute.min(59),
            duration_secs,
            enabled: true,
            label: label.to_string(),
            one_shot: true,
        }
    }

    /// Human-readable start time display.
    pub fn start_time_display(&self) -> String {
        format!("{:02}:{:02}", self.start_hour, self.start_minute)
    }

    /// Human-readable duration display.
    pub fn duration_display(&self) -> String {
        if self.duration_secs == 0 {
            "Until stopped".to_string()
        } else {
            format_duration(self.duration_secs)
        }
    }

    /// Whether the schedule should trigger at the given time.
    pub fn should_trigger(&self, hour: u8, minute: u8) -> bool {
        self.enabled && self.start_hour == hour && self.start_minute == minute
    }

    /// Mark as triggered (disable if one-shot).
    pub fn mark_triggered(&mut self) {
        if self.one_shot {
            self.enabled = false;
        }
    }
}

/// Schedule manager.
#[derive(Clone, Debug, Default)]
pub struct ScheduleManager {
    pub schedules: Vec<ScheduledRecording>,
    next_id: u32,
}

impl ScheduleManager {
    /// Create an empty schedule manager.
    pub fn new() -> Self {
        Self {
            schedules: Vec::new(),
            next_id: 1,
        }
    }

    /// Add a new schedule, returning its assigned ID.
    pub fn add(&mut self, mut schedule: ScheduledRecording) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        schedule.id = id;
        self.schedules.push(schedule);
        id
    }

    /// Remove a schedule by ID.
    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.schedules.len();
        self.schedules.retain(|s| s.id != id);
        self.schedules.len() != before
    }

    /// Get a schedule by ID.
    pub fn get(&self, id: u32) -> Option<&ScheduledRecording> {
        self.schedules.iter().find(|s| s.id == id)
    }

    /// Get a mutable reference to a schedule by ID.
    pub fn get_mut(&mut self, id: u32) -> Option<&mut ScheduledRecording> {
        self.schedules.iter_mut().find(|s| s.id == id)
    }

    /// Check if any schedule should trigger at the given time.
    pub fn check_triggers(&self, hour: u8, minute: u8) -> Vec<u32> {
        self.schedules
            .iter()
            .filter(|s| s.should_trigger(hour, minute))
            .map(|s| s.id)
            .collect()
    }

    /// Number of active (enabled) schedules.
    pub fn active_count(&self) -> usize {
        self.schedules.iter().filter(|s| s.enabled).count()
    }

    /// Total number of schedules.
    pub fn len(&self) -> usize {
        self.schedules.len()
    }

    /// Whether there are no schedules.
    pub fn is_empty(&self) -> bool {
        self.schedules.is_empty()
    }
}

// ============================================================================
// Region selector
// ============================================================================

/// State for the custom region selection overlay.
#[derive(Clone, Debug)]
pub struct RegionSelector {
    /// Whether region selection mode is active.
    pub active: bool,
    /// Whether the user is currently dragging.
    pub dragging: bool,
    /// Drag start position.
    pub start_x: f32,
    pub start_y: f32,
    /// Current mouse position during drag.
    pub current_x: f32,
    pub current_y: f32,
    /// Screen dimensions.
    pub screen_width: f32,
    pub screen_height: f32,
}

impl RegionSelector {
    /// Create a new region selector for the given screen dimensions.
    pub fn new(screen_width: f32, screen_height: f32) -> Self {
        Self {
            active: false,
            dragging: false,
            start_x: 0.0,
            start_y: 0.0,
            current_x: 0.0,
            current_y: 0.0,
            screen_width,
            screen_height,
        }
    }

    /// Begin region selection mode.
    pub fn activate(&mut self) {
        self.active = true;
        self.dragging = false;
    }

    /// Start dragging from the given position.
    pub fn begin_drag(&mut self, x: f32, y: f32) {
        self.dragging = true;
        self.start_x = x;
        self.start_y = y;
        self.current_x = x;
        self.current_y = y;
    }

    /// Update the drag position.
    pub fn update_drag(&mut self, x: f32, y: f32) {
        if self.dragging {
            self.current_x = x;
            self.current_y = y;
        }
    }

    /// End the drag and return the selected region as (x, y, width, height).
    pub fn end_drag(&mut self) -> Option<(f32, f32, f32, f32)> {
        if !self.dragging {
            return None;
        }
        self.dragging = false;
        self.active = false;

        let x = self.start_x.min(self.current_x);
        let y = self.start_y.min(self.current_y);
        let w = (self.current_x - self.start_x).abs();
        let h = (self.current_y - self.start_y).abs();

        if w < 10.0 || h < 10.0 {
            // Region too small, treat as cancelled
            return None;
        }

        Some((x, y, w, h))
    }

    /// Cancel region selection.
    pub fn cancel(&mut self) {
        self.active = false;
        self.dragging = false;
    }

    /// The current selected rectangle (min_x, min_y, width, height) during drag.
    pub fn current_rect(&self) -> (f32, f32, f32, f32) {
        let x = self.start_x.min(self.current_x);
        let y = self.start_y.min(self.current_y);
        let w = (self.current_x - self.start_x).abs();
        let h = (self.current_y - self.start_y).abs();
        (x, y, w, h)
    }

    /// Render the region selection overlay.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        if !self.active {
            return cmds;
        }

        // Dim overlay over entire screen
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.screen_width,
            height: self.screen_height,
            color: Color::rgba(0, 0, 0, 120),
            corner_radii: CornerRadii::ZERO,
        });

        if self.dragging {
            let (rx, ry, rw, rh) = self.current_rect();
            // Clear the selected region (draw bright rect over the dim)
            cmds.push(RenderCommand::FillRect {
                x: rx,
                y: ry,
                width: rw,
                height: rh,
                color: Color::rgba(0, 0, 0, 0),
                corner_radii: CornerRadii::ZERO,
            });
            // Selection border
            cmds.push(RenderCommand::StrokeRect {
                x: rx,
                y: ry,
                width: rw,
                height: rh,
                color: colors::BLUE,
                line_width: 2.0,
                corner_radii: CornerRadii::ZERO,
            });
            // Dimension label
            let label = format!("{}x{}", rw as u32, rh as u32);
            cmds.push(RenderCommand::Text {
                x: rx + rw / 2.0 - 30.0,
                y: ry + rh + 4.0,
                text: label,
                font_size: 12.0,
                color: colors::TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        cmds
    }
}

// ============================================================================
// BMP frame encoder
// ============================================================================

/// Errors that can occur during BMP encoding.
#[derive(Debug)]
pub enum BmpError {
    /// Pixel buffer size does not match width * height.
    PixelCountMismatch { expected: usize, actual: usize },
    /// I/O error writing the file.
    Io(std::io::Error),
    /// Dimensions overflow BMP format limits.
    DimensionOverflow,
}

impl From<std::io::Error> for BmpError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl core::fmt::Display for BmpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::PixelCountMismatch { expected, actual } => {
                write!(f, "pixel count mismatch: expected {expected}, got {actual}")
            }
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::DimensionOverflow => write!(f, "image dimensions overflow BMP format limits"),
        }
    }
}

/// Encode pixel data as a 32-bit BMP byte buffer.
///
/// Pixel data is ARGB (u32 per pixel), row-major, top-down.
/// BMP stores rows bottom-up with BGRA byte order.
pub fn encode_bmp_frame(width: u32, height: u32, pixels: &[u32]) -> Result<Vec<u8>, BmpError> {
    let expected = (width as usize).saturating_mul(height as usize);
    if pixels.len() != expected {
        return Err(BmpError::PixelCountMismatch {
            expected,
            actual: pixels.len(),
        });
    }

    let row_bytes = width
        .checked_mul(BMP_BYTES_PER_PIXEL)
        .ok_or(BmpError::DimensionOverflow)?;
    let pixel_data_size = row_bytes
        .checked_mul(height)
        .ok_or(BmpError::DimensionOverflow)?;
    let header_size = BMP_FILE_HEADER_SIZE + BMP_INFO_HEADER_SIZE;
    let file_size = header_size
        .checked_add(pixel_data_size)
        .ok_or(BmpError::DimensionOverflow)?;

    let mut buf = Vec::with_capacity(file_size as usize);

    // BITMAPFILEHEADER (14 bytes)
    buf.extend_from_slice(b"BM");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&header_size.to_le_bytes());

    // BITMAPINFOHEADER (40 bytes)
    buf.extend_from_slice(&BMP_INFO_HEADER_SIZE.to_le_bytes());
    buf.extend_from_slice(&(width as i32).to_le_bytes());
    buf.extend_from_slice(&(height as i32).to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // planes
    buf.extend_from_slice(&32u16.to_le_bytes()); // bpp
    buf.extend_from_slice(&0u32.to_le_bytes()); // compression
    buf.extend_from_slice(&pixel_data_size.to_le_bytes());
    buf.extend_from_slice(&2835i32.to_le_bytes()); // X ppm
    buf.extend_from_slice(&2835i32.to_le_bytes()); // Y ppm
    buf.extend_from_slice(&0u32.to_le_bytes()); // colors used
    buf.extend_from_slice(&0u32.to_le_bytes()); // important colors

    // Pixel data (bottom-up, BGRA)
    for y in (0..height).rev() {
        let row_start = (y as usize).saturating_mul(width as usize);
        for x in 0..width as usize {
            let idx = row_start.saturating_add(x);
            let argb = pixels.get(idx).copied().unwrap_or(0);
            let a = ((argb >> 24) & 0xFF) as u8;
            let r = ((argb >> 16) & 0xFF) as u8;
            let g_val = ((argb >> 8) & 0xFF) as u8;
            let b = (argb & 0xFF) as u8;
            buf.push(b);
            buf.push(g_val);
            buf.push(r);
            buf.push(a);
        }
    }

    Ok(buf)
}

// ============================================================================
// Recording indicator (floating overlay)
// ============================================================================

/// Floating indicator overlay showing recording status.
#[derive(Clone, Debug)]
pub struct RecordingIndicator {
    /// Position on screen.
    pub x: f32,
    pub y: f32,
    /// Elapsed recording time in seconds.
    pub elapsed_secs: u64,
    /// Current file size in bytes.
    pub file_size: u64,
    /// Current FPS being achieved.
    pub current_fps: u32,
    /// Whether the indicator is visible.
    pub visible: bool,
    /// Whether the recording dot should blink (for visual pulse).
    pub blink_on: bool,
}

impl RecordingIndicator {
    /// Create a new indicator at the given position.
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            elapsed_secs: 0,
            file_size: 0,
            current_fps: 0,
            visible: true,
            blink_on: true,
        }
    }

    /// Update elapsed time and file size.
    pub fn update(&mut self, elapsed_secs: u64, file_size: u64, fps: u32) {
        self.elapsed_secs = elapsed_secs;
        self.file_size = file_size;
        self.current_fps = fps;
    }

    /// Toggle the blink state (called on a timer).
    pub fn toggle_blink(&mut self) {
        self.blink_on = !self.blink_on;
    }

    /// Render the indicator overlay.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        if !self.visible {
            return cmds;
        }

        let indicator_width: f32 = 260.0;
        let indicator_height: f32 = 36.0;

        // Background with shadow
        cmds.push(RenderCommand::BoxShadow {
            x: self.x,
            y: self.y,
            width: indicator_width,
            height: indicator_height,
            offset_x: 0.0,
            offset_y: 2.0,
            blur: 8.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 120),
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.push(RenderCommand::FillRect {
            x: self.x,
            y: self.y,
            width: indicator_width,
            height: indicator_height,
            color: colors::CRUST,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });

        // Recording dot (blinks)
        if self.blink_on {
            cmds.push(RenderCommand::FillRect {
                x: self.x + 10.0,
                y: self.y + 12.0,
                width: 12.0,
                height: 12.0,
                color: colors::RED,
                corner_radii: CornerRadii::all(6.0),
            });
        }

        // Elapsed time
        cmds.push(RenderCommand::Text {
            x: self.x + 28.0,
            y: self.y + 10.0,
            text: format_duration(self.elapsed_secs),
            font_size: 14.0,
            color: colors::TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // File size
        cmds.push(RenderCommand::Text {
            x: self.x + 105.0,
            y: self.y + 10.0,
            text: format_file_size(self.file_size),
            font_size: 12.0,
            color: colors::SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // FPS
        cmds.push(RenderCommand::Text {
            x: self.x + 200.0,
            y: self.y + 10.0,
            text: format!("{} fps", self.current_fps),
            font_size: 12.0,
            color: colors::OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds
    }
}

// ============================================================================
// Active view / tab
// ============================================================================

/// Which view/tab is currently shown in the main area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveView {
    /// Main recording controls and preview.
    Record,
    /// Recording history list.
    History,
    /// Trim tool for the selected recording.
    Trim,
    /// Settings panel (output, hotkeys, schedules).
    Settings,
}

impl ActiveView {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Record => "Record",
            Self::History => "History",
            Self::Trim => "Trim",
            Self::Settings => "Settings",
        }
    }

    /// All available views.
    pub fn all() -> &'static [ActiveView] {
        &[Self::Record, Self::History, Self::Trim, Self::Settings]
    }
}

// ============================================================================
// Settings sub-tab
// ============================================================================

/// Which settings sub-section is currently visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTab {
    Output,
    Hotkeys,
    Schedule,
}

impl SettingsTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Output => "Output",
            Self::Hotkeys => "Hotkeys",
            Self::Schedule => "Schedule",
        }
    }

    pub fn all() -> &'static [SettingsTab] {
        &[Self::Output, Self::Hotkeys, Self::Schedule]
    }
}

// ============================================================================
// Main application state
// ============================================================================

/// Top-level screen recorder application state.
pub struct ScreenRecorderApp {
    /// Current recording state.
    pub recording_state: RecordingState,
    /// Current capture mode.
    pub capture_mode: CaptureMode,
    /// Selected FPS preset.
    pub fps_preset: FpsPreset,
    /// Audio capture settings.
    pub audio: AudioSettings,
    /// Countdown timer for pre-recording delay.
    pub countdown: CountdownTimer,
    /// Mouse cursor capture options.
    pub cursor_options: CursorOptions,
    /// Active annotation tool (None if annotations are off).
    pub active_annotation_tool: Option<AnnotationTool>,
    /// Annotations drawn during the current recording.
    pub annotations: Vec<Annotation>,
    /// Currently-in-progress annotation (being drawn via drag).
    pub current_annotation: Option<Annotation>,
    /// Output / save settings.
    pub output: OutputSettings,
    /// Recording history.
    pub history: RecordingHistory,
    /// Trim range for the currently selected recording.
    pub trim: Option<TrimRange>,
    /// Hotkey bindings.
    pub hotkeys: Vec<HotkeyBinding>,
    /// Scheduled recordings.
    pub schedules: ScheduleManager,
    /// Region selector state.
    pub region_selector: RegionSelector,
    /// Recording indicator overlay.
    pub indicator: RecordingIndicator,
    /// Which view is currently active.
    pub active_view: ActiveView,
    /// Which settings sub-tab is active.
    pub settings_tab: SettingsTab,
    /// Window dimensions.
    pub window_width: f32,
    pub window_height: f32,
    /// Total frames recorded in the current session.
    pub total_frames: u32,
    /// Total bytes written in the current session.
    pub total_bytes: u64,
    /// Recording elapsed time in seconds.
    pub elapsed_secs: u64,
    /// Custom region if one has been selected: (x, y, w, h).
    pub selected_region: Option<(f32, f32, f32, f32)>,
    /// Hovered sidebar item index (for hover effects).
    pub hovered_sidebar: Option<usize>,
    /// Whether the annotation toolbar is expanded.
    pub annotation_toolbar_visible: bool,
}

impl ScreenRecorderApp {
    /// Create a new application instance with default settings.
    pub fn new() -> Self {
        Self {
            recording_state: RecordingState::Idle,
            capture_mode: CaptureMode::FullScreen,
            fps_preset: FpsPreset::Fps30,
            audio: AudioSettings::default(),
            countdown: CountdownTimer::new(3),
            cursor_options: CursorOptions::default(),
            active_annotation_tool: None,
            annotations: Vec::new(),
            current_annotation: None,
            output: OutputSettings::default(),
            history: RecordingHistory::mock_entries(),
            trim: None,
            hotkeys: default_hotkeys(),
            schedules: ScheduleManager::new(),
            region_selector: RegionSelector::new(1920.0, 1080.0),
            indicator: RecordingIndicator::new(20.0, 20.0),
            active_view: ActiveView::Record,
            settings_tab: SettingsTab::Output,
            window_width: WINDOW_WIDTH,
            window_height: WINDOW_HEIGHT,
            total_frames: 0,
            total_bytes: 0,
            elapsed_secs: 0,
            selected_region: None,
            hovered_sidebar: None,
            annotation_toolbar_visible: false,
        }
    }

    /// Start a new recording (with countdown if configured).
    pub fn start_recording(&mut self) {
        if self.recording_state != RecordingState::Idle {
            return;
        }
        if self.countdown.duration_secs > 0 {
            self.recording_state = RecordingState::Countdown;
            self.countdown.start();
        } else {
            self.recording_state = RecordingState::Recording;
        }
        self.total_frames = 0;
        self.total_bytes = 0;
        self.elapsed_secs = 0;
        self.annotations.clear();
        self.indicator.visible = true;
    }

    /// Stop the current recording.
    pub fn stop_recording(&mut self) {
        if !self.recording_state.is_active() {
            return;
        }
        self.recording_state = RecordingState::Stopped;
        self.indicator.visible = false;
        self.countdown.cancel();
    }

    /// Pause the recording.
    pub fn pause_recording(&mut self) {
        if self.recording_state == RecordingState::Recording {
            self.recording_state = RecordingState::Paused;
        }
    }

    /// Resume a paused recording.
    pub fn resume_recording(&mut self) {
        if self.recording_state == RecordingState::Paused {
            self.recording_state = RecordingState::Recording;
        }
    }

    /// Reset to idle state (discard current recording).
    pub fn reset(&mut self) {
        self.recording_state = RecordingState::Idle;
        self.total_frames = 0;
        self.total_bytes = 0;
        self.elapsed_secs = 0;
        self.annotations.clear();
        self.current_annotation = None;
        self.indicator.visible = false;
        self.countdown.reset();
    }

    /// Process a countdown tick. Returns true if the countdown finished
    /// and recording has started.
    pub fn tick_countdown(&mut self) -> bool {
        if self.recording_state != RecordingState::Countdown {
            return false;
        }
        if self.countdown.tick() {
            self.recording_state = RecordingState::Recording;
            return true;
        }
        false
    }

    /// Record a frame (increment counters). Returns the frame number.
    pub fn record_frame(&mut self, frame_bytes: u64) -> u32 {
        if self.recording_state != RecordingState::Recording {
            return self.total_frames;
        }
        self.total_frames = self.total_frames.saturating_add(1);
        self.total_bytes = self.total_bytes.saturating_add(frame_bytes);
        self.indicator.update(self.elapsed_secs, self.total_bytes, self.fps_preset.value());
        self.total_frames
    }

    /// Advance elapsed time by one second (called from a timer).
    pub fn tick_elapsed(&mut self) {
        if self.recording_state == RecordingState::Recording {
            self.elapsed_secs = self.elapsed_secs.saturating_add(1);
            self.indicator.update(self.elapsed_secs, self.total_bytes, self.fps_preset.value());
        }
    }

    /// Save the current recording to history.
    pub fn save_to_history(&mut self) -> u32 {
        let entry = HistoryEntry::new(
            0,
            self.output.generate_filename(2026, 5, 18, 12, 0, 0),
            self.output.generate_path(2026, 5, 18, 12, 0, 0),
            self.elapsed_secs,
            self.total_bytes,
            self.total_frames,
            self.fps_preset.value(),
            1920,
            1080,
        )
        .with_timestamp(2026, 5, 18, 12, 0, 0);

        let id = self.history.add(entry);
        self.output.bump_increment();
        id
    }

    /// Set up a trim range for the currently selected history entry.
    pub fn setup_trim_for_selected(&mut self) -> bool {
        if let Some(entry) = self.history.selected() {
            self.trim = Some(TrimRange::new(entry.duration_secs as f64));
            self.active_view = ActiveView::Trim;
            true
        } else {
            false
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the full application UI.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Main background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: colors::BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Render sidebar
        cmds.extend(self.render_sidebar());

        // Render toolbar
        cmds.extend(self.render_toolbar());

        // Render main content area based on active view
        let content_x = SIDEBAR_WIDTH;
        let content_y = TOOLBAR_HEIGHT;
        let content_w = self.window_width - SIDEBAR_WIDTH;
        let content_h = self.window_height - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT;

        cmds.push(RenderCommand::PushClip {
            x: content_x,
            y: content_y,
            width: content_w,
            height: content_h,
        });

        match self.active_view {
            ActiveView::Record => cmds.extend(self.render_record_view(content_x, content_y, content_w, content_h)),
            ActiveView::History => cmds.extend(self.render_history_view(content_x, content_y, content_w, content_h)),
            ActiveView::Trim => cmds.extend(self.render_trim_view(content_x, content_y, content_w, content_h)),
            ActiveView::Settings => cmds.extend(self.render_settings_view(content_x, content_y, content_w, content_h)),
        }

        cmds.push(RenderCommand::PopClip);

        // Status bar
        cmds.extend(self.render_status_bar());

        // Overlays (region selector, recording indicator, countdown)
        if self.region_selector.active {
            cmds.extend(self.region_selector.render());
        }
        if self.recording_state == RecordingState::Recording
            || self.recording_state == RecordingState::Paused
        {
            cmds.extend(self.indicator.render());
        }
        if self.recording_state == RecordingState::Countdown {
            cmds.extend(self.render_countdown_overlay());
        }

        cmds
    }

    /// Render the left sidebar with navigation.
    fn render_sidebar(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Sidebar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: SIDEBAR_WIDTH,
            height: self.window_height,
            color: colors::MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // App title
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 14.0,
            text: "Screen Recorder".to_string(),
            font_size: 16.0,
            color: colors::LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0),
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: PADDING,
            y1: 42.0,
            x2: SIDEBAR_WIDTH - PADDING,
            y2: 42.0,
            color: colors::SURFACE0,
            width: 1.0,
        });

        // Navigation items
        let views = ActiveView::all();
        let nav_start_y: f32 = 52.0;
        let item_height: f32 = 36.0;

        for (i, view) in views.iter().enumerate() {
            let y = nav_start_y + (i as f32) * item_height;
            let is_active = self.active_view == *view;
            let is_hovered = self.hovered_sidebar == Some(i);

            // Highlight background for active/hovered
            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: 4.0,
                    y,
                    width: SIDEBAR_WIDTH - 8.0,
                    height: item_height - 2.0,
                    color: colors::SURFACE0,
                    corner_radii: CornerRadii::all(SMALL_RADIUS),
                });
                // Active indicator bar
                cmds.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: y + 6.0,
                    width: 3.0,
                    height: item_height - 14.0,
                    color: colors::BLUE,
                    corner_radii: CornerRadii::all(1.5),
                });
            } else if is_hovered {
                cmds.push(RenderCommand::FillRect {
                    x: 4.0,
                    y,
                    width: SIDEBAR_WIDTH - 8.0,
                    height: item_height - 2.0,
                    color: colors::SURFACE1,
                    corner_radii: CornerRadii::all(SMALL_RADIUS),
                });
            }

            let text_color = if is_active { colors::TEXT } else { colors::SUBTEXT0 };
            cmds.push(RenderCommand::Text {
                x: PADDING + 8.0,
                y: y + 9.0,
                text: view.label().to_string(),
                font_size: 14.0,
                color: text_color,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0 - 8.0),
            });
        }

        // Recording state badge at bottom of sidebar
        let badge_y = self.window_height - 80.0;
        cmds.push(RenderCommand::Line {
            x1: PADDING,
            y1: badge_y - 8.0,
            x2: SIDEBAR_WIDTH - PADDING,
            y2: badge_y - 8.0,
            color: colors::SURFACE0,
            width: 1.0,
        });

        // State dot
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: badge_y + 4.0,
            width: 10.0,
            height: 10.0,
            color: self.recording_state.color(),
            corner_radii: CornerRadii::all(5.0),
        });

        cmds.push(RenderCommand::Text {
            x: PADDING + 16.0,
            y: badge_y,
            text: self.recording_state.label().to_string(),
            font_size: 13.0,
            color: self.recording_state.color(),
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0 - 16.0),
        });

        // Capture mode info
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: badge_y + 22.0,
            text: format!("{} | {}", self.capture_mode.label(), self.fps_preset.label()),
            font_size: 11.0,
            color: colors::OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0),
        });

        cmds
    }

    /// Render the top toolbar.
    fn render_toolbar(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Toolbar background
        cmds.push(RenderCommand::FillRect {
            x: SIDEBAR_WIDTH,
            y: 0.0,
            width: self.window_width - SIDEBAR_WIDTH,
            height: TOOLBAR_HEIGHT,
            color: colors::MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Bottom border
        cmds.push(RenderCommand::Line {
            x1: SIDEBAR_WIDTH,
            y1: TOOLBAR_HEIGHT - 1.0,
            x2: self.window_width,
            y2: TOOLBAR_HEIGHT - 1.0,
            color: colors::SURFACE0,
            width: 1.0,
        });

        let btn_y = (TOOLBAR_HEIGHT - BUTTON_HEIGHT) / 2.0;
        let mut btn_x = SIDEBAR_WIDTH + PADDING;

        // Record / Stop button
        let (rec_label, rec_color) = if self.recording_state.is_active() {
            ("Stop", colors::RED)
        } else {
            ("Record", colors::GREEN)
        };

        cmds.push(RenderCommand::FillRect {
            x: btn_x,
            y: btn_y,
            width: 90.0,
            height: BUTTON_HEIGHT,
            color: rec_color,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: btn_x + 22.0,
            y: btn_y + 9.0,
            text: rec_label.to_string(),
            font_size: 13.0,
            color: colors::CRUST,
            font_weight: FontWeightHint::Bold,
            max_width: Some(66.0),
        });
        btn_x += 90.0 + BUTTON_SPACING;

        // Pause / Resume button (only when recording or paused)
        if self.recording_state == RecordingState::Recording
            || self.recording_state == RecordingState::Paused
        {
            let pause_label = if self.recording_state == RecordingState::Paused {
                "Resume"
            } else {
                "Pause"
            };
            cmds.push(RenderCommand::FillRect {
                x: btn_x,
                y: btn_y,
                width: 80.0,
                height: BUTTON_HEIGHT,
                color: colors::SURFACE1,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: btn_x + 14.0,
                y: btn_y + 9.0,
                text: pause_label.to_string(),
                font_size: 13.0,
                color: colors::TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(52.0),
            });
            btn_x += 80.0 + BUTTON_SPACING;
        }

        // Annotation toggle
        if self.recording_state == RecordingState::Recording {
            let ann_color = if self.annotation_toolbar_visible {
                colors::BLUE
            } else {
                colors::SURFACE1
            };
            cmds.push(RenderCommand::FillRect {
                x: btn_x,
                y: btn_y,
                width: 90.0,
                height: BUTTON_HEIGHT,
                color: ann_color,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: btn_x + 12.0,
                y: btn_y + 9.0,
                text: "Annotate".to_string(),
                font_size: 13.0,
                color: if self.annotation_toolbar_visible {
                    colors::CRUST
                } else {
                    colors::TEXT
                },
                font_weight: FontWeightHint::Regular,
                max_width: Some(66.0),
            });
        }

        // Right-aligned: capture mode selector
        let mode_x = self.window_width - 180.0;
        cmds.push(RenderCommand::Text {
            x: mode_x,
            y: btn_y + 9.0,
            text: self.capture_mode.label().to_string(),
            font_size: 12.0,
            color: colors::SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(170.0),
        });

        cmds
    }

    /// Render the main recording view.
    fn render_record_view(&self, x: f32, y: f32, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        let cx = x + PADDING;
        let mut cy = y + PADDING;

        // Preview area (dark box simulating the capture area)
        let preview_w = width - PADDING * 2.0;
        let preview_h = height * 0.55;

        cmds.push(RenderCommand::FillRect {
            x: cx,
            y: cy,
            width: preview_w,
            height: preview_h,
            color: colors::CRUST,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: cx,
            y: cy,
            width: preview_w,
            height: preview_h,
            color: colors::SURFACE0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Preview label
        let preview_label = match self.recording_state {
            RecordingState::Idle => "Preview",
            RecordingState::Countdown => "Starting...",
            RecordingState::Recording => "Recording",
            RecordingState::Paused => "Paused",
            RecordingState::Stopped => "Review",
        };
        cmds.push(RenderCommand::Text {
            x: cx + preview_w / 2.0 - 30.0,
            y: cy + preview_h / 2.0 - 8.0,
            text: preview_label.to_string(),
            font_size: 16.0,
            color: colors::OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(preview_w),
        });

        // Render annotations over preview
        for ann in &self.annotations {
            cmds.extend(ann.render());
        }
        if let Some(ref ann) = self.current_annotation {
            cmds.extend(ann.render());
        }

        cy += preview_h + SECTION_SPACING;

        // Annotation toolbar (below preview)
        if self.annotation_toolbar_visible && self.recording_state == RecordingState::Recording {
            cmds.extend(self.render_annotation_toolbar(cx, cy, preview_w));
            cy += 42.0 + BUTTON_SPACING;
        }

        // Recording settings panel
        cmds.extend(self.render_recording_settings(cx, cy, preview_w));

        cmds
    }

    /// Render the annotation tool bar.
    fn render_annotation_toolbar(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: 40.0,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });

        let tools = AnnotationTool::all();
        let tool_btn_w: f32 = 80.0;
        let mut tx = x + BUTTON_SPACING;

        for tool in tools {
            let is_active = self.active_annotation_tool == Some(*tool);
            let bg = if is_active {
                colors::BLUE
            } else {
                colors::SURFACE1
            };
            let fg = if is_active {
                colors::CRUST
            } else {
                colors::TEXT
            };

            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: y + 5.0,
                width: tool_btn_w,
                height: 30.0,
                color: bg,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 10.0,
                y: y + 12.0,
                text: tool.label().to_string(),
                font_size: 12.0,
                color: fg,
                font_weight: FontWeightHint::Regular,
                max_width: Some(tool_btn_w - 20.0),
            });
            tx += tool_btn_w + BUTTON_SPACING;
        }

        cmds
    }

    /// Render the recording settings below the preview.
    fn render_recording_settings(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Section title
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: "Capture Settings".to_string(),
            font_size: 14.0,
            color: colors::TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });

        let row_y = y + 24.0;
        let col_w = width / 3.0;

        // Capture mode
        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Mode".to_string(),
            font_size: 11.0,
            color: colors::OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(col_w),
        });
        cmds.push(RenderCommand::Text {
            x,
            y: row_y + 16.0,
            text: self.capture_mode.label().to_string(),
            font_size: 13.0,
            color: colors::TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(col_w),
        });

        // FPS
        cmds.push(RenderCommand::Text {
            x: x + col_w,
            y: row_y,
            text: "Frame Rate".to_string(),
            font_size: 11.0,
            color: colors::OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(col_w),
        });
        cmds.push(RenderCommand::Text {
            x: x + col_w,
            y: row_y + 16.0,
            text: self.fps_preset.label().to_string(),
            font_size: 13.0,
            color: colors::TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(col_w),
        });

        // Audio
        let audio_text = if self.audio.any_audio_enabled() {
            let mut parts = Vec::new();
            if self.audio.system_audio_enabled {
                parts.push(format!("Sys {}%", self.audio.system_volume_percent()));
            }
            if self.audio.microphone_enabled {
                parts.push(format!("Mic {}%", self.audio.microphone_volume_percent()));
            }
            parts.join(", ")
        } else {
            "No Audio".to_string()
        };

        cmds.push(RenderCommand::Text {
            x: x + col_w * 2.0,
            y: row_y,
            text: "Audio".to_string(),
            font_size: 11.0,
            color: colors::OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(col_w),
        });
        cmds.push(RenderCommand::Text {
            x: x + col_w * 2.0,
            y: row_y + 16.0,
            text: audio_text,
            font_size: 13.0,
            color: colors::TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(col_w),
        });

        // Cursor options row
        let row2_y = row_y + 44.0;
        cmds.push(RenderCommand::Text {
            x,
            y: row2_y,
            text: "Cursor".to_string(),
            font_size: 11.0,
            color: colors::OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(col_w),
        });
        let cursor_text = if self.cursor_options.capture_cursor {
            if self.cursor_options.click_highlight {
                "Visible + Highlight"
            } else {
                "Visible"
            }
        } else {
            "Hidden"
        };
        cmds.push(RenderCommand::Text {
            x,
            y: row2_y + 16.0,
            text: cursor_text.to_string(),
            font_size: 13.0,
            color: colors::TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(col_w),
        });

        // Countdown
        cmds.push(RenderCommand::Text {
            x: x + col_w,
            y: row2_y,
            text: "Countdown".to_string(),
            font_size: 11.0,
            color: colors::OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(col_w),
        });
        cmds.push(RenderCommand::Text {
            x: x + col_w,
            y: row2_y + 16.0,
            text: format!("{}s", self.countdown.duration_secs),
            font_size: 13.0,
            color: colors::TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(col_w),
        });

        // Output directory
        cmds.push(RenderCommand::Text {
            x: x + col_w * 2.0,
            y: row2_y,
            text: "Save to".to_string(),
            font_size: 11.0,
            color: colors::OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(col_w),
        });
        let dir_display = self.output.save_directory.to_string_lossy().to_string();
        cmds.push(RenderCommand::Text {
            x: x + col_w * 2.0,
            y: row2_y + 16.0,
            text: dir_display,
            font_size: 13.0,
            color: colors::TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(col_w),
        });

        cmds
    }

    /// Render the history view.
    fn render_history_view(&self, x: f32, y: f32, width: f32, _height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        cmds.push(RenderCommand::Text {
            x: x + PADDING,
            y: y + PADDING,
            text: "Recording History".to_string(),
            font_size: 16.0,
            color: colors::TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - PADDING * 2.0),
        });

        // Summary line
        cmds.push(RenderCommand::Text {
            x: x + PADDING,
            y: y + PADDING + 24.0,
            text: format!(
                "{} recordings, {} total",
                self.history.len(),
                format_file_size(self.history.total_size())
            ),
            font_size: 12.0,
            color: colors::OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - PADDING * 2.0),
        });

        if self.history.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: y + 80.0,
                text: "No recordings yet.".to_string(),
                font_size: 14.0,
                color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PADDING * 2.0),
            });
            return cmds;
        }

        let entry_height: f32 = 64.0;
        let list_x = x + PADDING;
        let list_w = width - PADDING * 2.0;
        let mut entry_y = y + 54.0;

        for entry in &self.history.entries {
            let bg_color = if entry.selected {
                colors::SURFACE0
            } else {
                colors::MANTLE
            };

            cmds.push(RenderCommand::FillRect {
                x: list_x,
                y: entry_y,
                width: list_w,
                height: entry_height - 4.0,
                color: bg_color,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });

            if entry.selected {
                cmds.push(RenderCommand::StrokeRect {
                    x: list_x,
                    y: entry_y,
                    width: list_w,
                    height: entry_height - 4.0,
                    color: colors::BLUE,
                    line_width: 1.0,
                    corner_radii: CornerRadii::all(SMALL_RADIUS),
                });
            }

            // Thumbnail placeholder
            cmds.push(RenderCommand::FillRect {
                x: list_x + 8.0,
                y: entry_y + 8.0,
                width: 72.0,
                height: 44.0,
                color: colors::SURFACE1,
                corner_radii: CornerRadii::all(3.0),
            });

            // Name
            cmds.push(RenderCommand::Text {
                x: list_x + 88.0,
                y: entry_y + 8.0,
                text: entry.name.clone(),
                font_size: 13.0,
                color: colors::TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(list_w - 96.0),
            });

            // Details line
            cmds.push(RenderCommand::Text {
                x: list_x + 88.0,
                y: entry_y + 26.0,
                text: format!(
                    "{} | {} | {} | {} fps",
                    entry.duration_display(),
                    entry.size_display(),
                    entry.resolution_display(),
                    entry.fps,
                ),
                font_size: 11.0,
                color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(list_w - 96.0),
            });

            // Timestamp
            cmds.push(RenderCommand::Text {
                x: list_x + 88.0,
                y: entry_y + 42.0,
                text: entry.timestamp_display(),
                font_size: 10.0,
                color: colors::OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(list_w - 96.0),
            });

            entry_y += entry_height;
        }

        cmds
    }

    /// Render the trim view.
    fn render_trim_view(&self, x: f32, y: f32, width: f32, _height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        cmds.push(RenderCommand::Text {
            x: x + PADDING,
            y: y + PADDING,
            text: "Trim Recording".to_string(),
            font_size: 16.0,
            color: colors::TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - PADDING * 2.0),
        });

        if let Some(ref trim) = self.trim {
            let track_x = x + PADDING;
            let track_y = y + 60.0;
            let track_w = width - PADDING * 2.0;
            let track_h: f32 = 40.0;

            // Track background
            cmds.push(RenderCommand::FillRect {
                x: track_x,
                y: track_y,
                width: track_w,
                height: track_h,
                color: colors::SURFACE0,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });

            // Trimmed region highlight
            let start_frac = trim.start_fraction() as f32;
            let end_frac = trim.end_fraction() as f32;
            let sel_x = track_x + track_w * start_frac;
            let sel_w = track_w * (end_frac - start_frac);

            cmds.push(RenderCommand::FillRect {
                x: sel_x,
                y: track_y,
                width: sel_w,
                height: track_h,
                color: colors::SURFACE1,
                corner_radii: CornerRadii::ZERO,
            });

            // Start handle
            cmds.push(RenderCommand::FillRect {
                x: sel_x - 4.0,
                y: track_y - 4.0,
                width: 8.0,
                height: track_h + 8.0,
                color: colors::BLUE,
                corner_radii: CornerRadii::all(2.0),
            });

            // End handle
            cmds.push(RenderCommand::FillRect {
                x: sel_x + sel_w - 4.0,
                y: track_y - 4.0,
                width: 8.0,
                height: track_h + 8.0,
                color: colors::BLUE,
                corner_radii: CornerRadii::all(2.0),
            });

            // Time labels
            let start_display = format_duration(trim.start_secs as u64);
            let end_display = format_duration(trim.end_secs as u64);
            let dur_display = format_duration(trim.trimmed_duration() as u64);

            cmds.push(RenderCommand::Text {
                x: track_x,
                y: track_y + track_h + 8.0,
                text: format!("Start: {}", start_display),
                font_size: 12.0,
                color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(track_w / 3.0),
            });
            cmds.push(RenderCommand::Text {
                x: track_x + track_w / 3.0,
                y: track_y + track_h + 8.0,
                text: format!("Duration: {}", dur_display),
                font_size: 12.0,
                color: colors::TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(track_w / 3.0),
            });
            cmds.push(RenderCommand::Text {
                x: track_x + track_w * 2.0 / 3.0,
                y: track_y + track_h + 8.0,
                text: format!("End: {}", end_display),
                font_size: 12.0,
                color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(track_w / 3.0),
            });

            // Trim / Reset buttons
            let btn_y = track_y + track_h + 36.0;

            cmds.push(RenderCommand::FillRect {
                x: track_x,
                y: btn_y,
                width: 100.0,
                height: BUTTON_HEIGHT,
                color: colors::GREEN,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: track_x + 20.0,
                y: btn_y + 9.0,
                text: "Apply Trim".to_string(),
                font_size: 13.0,
                color: colors::CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(80.0),
            });

            cmds.push(RenderCommand::FillRect {
                x: track_x + 110.0,
                y: btn_y,
                width: 80.0,
                height: BUTTON_HEIGHT,
                color: colors::SURFACE1,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: track_x + 126.0,
                y: btn_y + 9.0,
                text: "Reset".to_string(),
                font_size: 13.0,
                color: colors::TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(60.0),
            });
        } else {
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: y + 60.0,
                text: "Select a recording from History to trim.".to_string(),
                font_size: 14.0,
                color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - PADDING * 2.0),
            });
        }

        cmds
    }

    /// Render the settings view.
    fn render_settings_view(&self, x: f32, y: f32, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Settings tabs
        let tab_y = y + PADDING;
        let tabs = SettingsTab::all();
        let tab_w: f32 = 90.0;
        let mut tx = x + PADDING;

        for tab in tabs {
            let is_active = self.settings_tab == *tab;
            let bg = if is_active {
                colors::SURFACE0
            } else {
                Color::TRANSPARENT
            };
            let fg = if is_active {
                colors::TEXT
            } else {
                colors::SUBTEXT0
            };

            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: tab_y,
                width: tab_w,
                height: 30.0,
                color: bg,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 12.0,
                y: tab_y + 8.0,
                text: tab.label().to_string(),
                font_size: 13.0,
                color: fg,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_w - 24.0),
            });
            tx += tab_w + 4.0;
        }

        let content_y = tab_y + 40.0;
        let content_h = height - 40.0 - PADDING;

        match self.settings_tab {
            SettingsTab::Output => {
                cmds.extend(self.render_output_settings(x + PADDING, content_y, width - PADDING * 2.0));
            }
            SettingsTab::Hotkeys => {
                cmds.extend(self.render_hotkey_settings(x + PADDING, content_y, width - PADDING * 2.0));
            }
            SettingsTab::Schedule => {
                cmds.extend(self.render_schedule_settings(x + PADDING, content_y, width - PADDING * 2.0, content_h));
            }
        }

        cmds
    }

    /// Render the output settings section.
    fn render_output_settings(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let mut cy = y;

        // Save directory
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Save Directory".to_string(),
            font_size: 12.0,
            color: colors::OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
        });
        cy += 18.0;
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: 30.0,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: cy + 8.0,
            text: self.output.save_directory.to_string_lossy().to_string(),
            font_size: 12.0,
            color: colors::TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 16.0),
        });
        cy += 40.0;

        // Filename template
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Filename Template".to_string(),
            font_size: 12.0,
            color: colors::OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
        });
        cy += 18.0;
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: 30.0,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: cy + 8.0,
            text: self.output.filename_template.clone(),
            font_size: 12.0,
            color: colors::TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 16.0),
        });
        cy += 40.0;

        // Auto-increment
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: format!("Auto-increment: #{:04}", self.output.auto_increment),
            font_size: 12.0,
            color: colors::SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
        });
        cy += 24.0;

        // Max file size
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: format!("Max File Size: {}", self.output.max_size_display()),
            font_size: 12.0,
            color: colors::SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
        });

        cmds
    }

    /// Render the hotkey settings section.
    fn render_hotkey_settings(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let row_height: f32 = 36.0;
        let mut cy = y;

        for binding in &self.hotkeys {
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: row_height - 4.0,
                color: colors::SURFACE0,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });

            // Action label
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: cy + 9.0,
                text: binding.action.label().to_string(),
                font_size: 13.0,
                color: colors::TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width / 2.0),
            });

            // Keybinding display
            cmds.push(RenderCommand::FillRect {
                x: x + width - 140.0,
                y: cy + 4.0,
                width: 130.0,
                height: 24.0,
                color: colors::SURFACE1,
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + width - 132.0,
                y: cy + 9.0,
                text: binding.display(),
                font_size: 12.0,
                color: colors::LAVENDER,
                font_weight: FontWeightHint::Bold,
                max_width: Some(122.0),
            });

            cy += row_height;
        }

        cmds
    }

    /// Render the schedule settings section.
    fn render_schedule_settings(
        &self,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        cmds.push(RenderCommand::Text {
            x,
            y,
            text: format!(
                "Scheduled Recordings ({} active)",
                self.schedules.active_count()
            ),
            font_size: 13.0,
            color: colors::TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });

        if self.schedules.is_empty() {
            cmds.push(RenderCommand::Text {
                x,
                y: y + 28.0,
                text: "No scheduled recordings. Click + to add one.".to_string(),
                font_size: 12.0,
                color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
            });

            // Add button
            cmds.push(RenderCommand::FillRect {
                x,
                y: y + 52.0,
                width: 120.0,
                height: BUTTON_HEIGHT,
                color: colors::BLUE,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: y + 61.0,
                text: "+ Add Schedule".to_string(),
                font_size: 12.0,
                color: colors::CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(100.0),
            });

            return cmds;
        }

        let row_h: f32 = 44.0;
        let mut cy = y + 28.0;

        for sched in &self.schedules.schedules {
            let bg = if sched.enabled {
                colors::SURFACE0
            } else {
                colors::CRUST
            };

            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: row_h - 4.0,
                color: bg,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });

            // Enable/disable indicator
            let dot_color = if sched.enabled {
                colors::GREEN
            } else {
                colors::OVERLAY0
            };
            cmds.push(RenderCommand::FillRect {
                x: x + 10.0,
                y: cy + 15.0,
                width: 10.0,
                height: 10.0,
                color: dot_color,
                corner_radii: CornerRadii::all(5.0),
            });

            // Label
            cmds.push(RenderCommand::Text {
                x: x + 28.0,
                y: cy + 6.0,
                text: sched.label.clone(),
                font_size: 13.0,
                color: if sched.enabled {
                    colors::TEXT
                } else {
                    colors::OVERLAY0
                },
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 180.0),
            });

            // Time and duration
            cmds.push(RenderCommand::Text {
                x: x + 28.0,
                y: cy + 22.0,
                text: format!(
                    "Start: {} | Duration: {}",
                    sched.start_time_display(),
                    sched.duration_display()
                ),
                font_size: 11.0,
                color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 180.0),
            });

            // Type badge
            let type_label = if sched.one_shot { "Once" } else { "Repeat" };
            cmds.push(RenderCommand::FillRect {
                x: x + width - 60.0,
                y: cy + 10.0,
                width: 50.0,
                height: 20.0,
                color: colors::SURFACE1,
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + width - 52.0,
                y: cy + 13.0,
                text: type_label.to_string(),
                font_size: 10.0,
                color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(42.0),
            });

            cy += row_h;
        }

        // Add button below the list
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy + 4.0,
            width: 120.0,
            height: BUTTON_HEIGHT,
            color: colors::BLUE,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: cy + 13.0,
            text: "+ Add Schedule".to_string(),
            font_size: 12.0,
            color: colors::CRUST,
            font_weight: FontWeightHint::Bold,
            max_width: Some(100.0),
        });

        cmds
    }

    /// Render the status bar at the bottom.
    fn render_status_bar(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        let bar_y = self.window_height - STATUS_BAR_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: bar_y,
            width: self.window_width,
            height: STATUS_BAR_HEIGHT,
            color: colors::CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Top border
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: bar_y,
            x2: self.window_width,
            y2: bar_y,
            color: colors::SURFACE0,
            width: 1.0,
        });

        let text_y = bar_y + 8.0;

        // Left: recording stats
        if self.recording_state.is_active() || self.recording_state == RecordingState::Stopped {
            cmds.push(RenderCommand::Text {
                x: PADDING,
                y: text_y,
                text: format!(
                    "{} | {} frames | {}",
                    format_duration(self.elapsed_secs),
                    self.total_frames,
                    format_file_size(self.total_bytes)
                ),
                font_size: 11.0,
                color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(self.window_width / 2.0),
            });
        } else {
            cmds.push(RenderCommand::Text {
                x: PADDING,
                y: text_y,
                text: "Ready to record".to_string(),
                font_size: 11.0,
                color: colors::OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(self.window_width / 2.0),
            });
        }

        // Right: hotkey hint
        cmds.push(RenderCommand::Text {
            x: self.window_width - 200.0,
            y: text_y,
            text: "F9: Start/Stop | F10: Pause".to_string(),
            font_size: 11.0,
            color: colors::OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(190.0),
        });

        cmds
    }

    /// Render the countdown overlay (large number centered on screen).
    fn render_countdown_overlay(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Semi-transparent background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: Color::rgba(0, 0, 0, 160),
            corner_radii: CornerRadii::ZERO,
        });

        // Countdown circle
        let cx = self.window_width / 2.0 - 50.0;
        let cy = self.window_height / 2.0 - 50.0;

        cmds.push(RenderCommand::FillRect {
            x: cx,
            y: cy,
            width: 100.0,
            height: 100.0,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::all(50.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: cx,
            y: cy,
            width: 100.0,
            height: 100.0,
            color: colors::BLUE,
            line_width: 3.0,
            corner_radii: CornerRadii::all(50.0),
        });

        // Countdown number
        cmds.push(RenderCommand::Text {
            x: cx + 35.0,
            y: cy + 28.0,
            text: format!("{}", self.countdown.remaining_secs),
            font_size: 40.0,
            color: colors::TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // "Get ready..." text below
        cmds.push(RenderCommand::Text {
            x: self.window_width / 2.0 - 45.0,
            y: cy + 120.0,
            text: "Get ready...".to_string(),
            font_size: 16.0,
            color: colors::SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds
    }
}

impl Default for ScreenRecorderApp {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    // Placeholder: the actual event loop is provided by the OS windowing system.
    // For now this validates that the application compiles and types are wired
    // together correctly.
    let _app = ScreenRecorderApp::new();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- CaptureMode tests --------------------------------------------------

    #[test]
    fn test_capture_mode_labels() {
        assert_eq!(CaptureMode::FullScreen.label(), "Full Screen");
        assert_eq!(CaptureMode::SelectedWindow.label(), "Selected Window");
        assert_eq!(CaptureMode::CustomRegion.label(), "Custom Region");
    }

    #[test]
    fn test_capture_mode_all() {
        let modes = CaptureMode::all();
        assert_eq!(modes.len(), 3);
        assert!(modes.contains(&CaptureMode::FullScreen));
        assert!(modes.contains(&CaptureMode::SelectedWindow));
        assert!(modes.contains(&CaptureMode::CustomRegion));
    }

    // -- FpsPreset tests ----------------------------------------------------

    #[test]
    fn test_fps_preset_values() {
        assert_eq!(FpsPreset::Fps15.value(), 15);
        assert_eq!(FpsPreset::Fps24.value(), 24);
        assert_eq!(FpsPreset::Fps30.value(), 30);
        assert_eq!(FpsPreset::Fps60.value(), 60);
    }

    #[test]
    fn test_fps_preset_labels() {
        assert_eq!(FpsPreset::Fps15.label(), "15 FPS");
        assert_eq!(FpsPreset::Fps24.label(), "24 FPS");
        assert_eq!(FpsPreset::Fps30.label(), "30 FPS");
        assert_eq!(FpsPreset::Fps60.label(), "60 FPS");
    }

    #[test]
    fn test_fps_frame_interval() {
        assert_eq!(FpsPreset::Fps15.frame_interval_ms(), 66);
        assert_eq!(FpsPreset::Fps30.frame_interval_ms(), 33);
        assert_eq!(FpsPreset::Fps60.frame_interval_ms(), 16);
    }

    #[test]
    fn test_fps_bytes_per_second() {
        // 1920x1080 at 30fps = 1920*1080*4*30 = 248,832,000
        let bps = FpsPreset::Fps30.bytes_per_second(1920, 1080);
        assert_eq!(bps, 248_832_000);
    }

    #[test]
    fn test_fps_all() {
        let all = FpsPreset::all();
        assert_eq!(all.len(), 4);
    }

    // -- RecordingState tests -----------------------------------------------

    #[test]
    fn test_state_idle_transitions() {
        assert!(RecordingState::Idle.can_transition_to(RecordingState::Countdown));
        assert!(RecordingState::Idle.can_transition_to(RecordingState::Recording));
        assert!(!RecordingState::Idle.can_transition_to(RecordingState::Paused));
        assert!(!RecordingState::Idle.can_transition_to(RecordingState::Stopped));
    }

    #[test]
    fn test_state_countdown_transitions() {
        assert!(RecordingState::Countdown.can_transition_to(RecordingState::Idle));
        assert!(!RecordingState::Countdown.can_transition_to(RecordingState::Recording));
        assert!(!RecordingState::Countdown.can_transition_to(RecordingState::Paused));
    }

    #[test]
    fn test_state_recording_transitions() {
        assert!(RecordingState::Recording.can_transition_to(RecordingState::Paused));
        assert!(RecordingState::Recording.can_transition_to(RecordingState::Stopped));
        assert!(!RecordingState::Recording.can_transition_to(RecordingState::Idle));
    }

    #[test]
    fn test_state_paused_transitions() {
        assert!(RecordingState::Paused.can_transition_to(RecordingState::Recording));
        assert!(RecordingState::Paused.can_transition_to(RecordingState::Stopped));
        assert!(!RecordingState::Paused.can_transition_to(RecordingState::Idle));
    }

    #[test]
    fn test_state_stopped_transitions() {
        assert!(RecordingState::Stopped.can_transition_to(RecordingState::Idle));
        assert!(!RecordingState::Stopped.can_transition_to(RecordingState::Recording));
    }

    #[test]
    fn test_state_labels() {
        assert_eq!(RecordingState::Idle.label(), "Ready");
        assert_eq!(RecordingState::Countdown.label(), "Countdown");
        assert_eq!(RecordingState::Recording.label(), "Recording");
        assert_eq!(RecordingState::Paused.label(), "Paused");
        assert_eq!(RecordingState::Stopped.label(), "Stopped");
    }

    #[test]
    fn test_state_colors_differ() {
        let idle = RecordingState::Idle.color();
        let recording = RecordingState::Recording.color();
        let paused = RecordingState::Paused.color();
        let stopped = RecordingState::Stopped.color();
        assert_ne!(idle, recording);
        assert_ne!(recording, paused);
        assert_ne!(paused, stopped);
    }

    #[test]
    fn test_state_is_active() {
        assert!(!RecordingState::Idle.is_active());
        assert!(RecordingState::Countdown.is_active());
        assert!(RecordingState::Recording.is_active());
        assert!(RecordingState::Paused.is_active());
        assert!(!RecordingState::Stopped.is_active());
    }

    // -- AudioSettings tests ------------------------------------------------

    #[test]
    fn test_audio_default() {
        let audio = AudioSettings::default();
        assert!(audio.system_audio_enabled);
        assert!(!audio.microphone_enabled);
        assert!((audio.system_volume - 0.8).abs() < f32::EPSILON);
        assert!((audio.microphone_volume - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn test_audio_new() {
        let audio = AudioSettings::new(false, true);
        assert!(!audio.system_audio_enabled);
        assert!(audio.microphone_enabled);
    }

    #[test]
    fn test_audio_volume_clamping() {
        let mut audio = AudioSettings::default();
        audio.set_system_volume(1.5);
        assert!((audio.system_volume - 1.0).abs() < f32::EPSILON);
        audio.set_system_volume(-0.5);
        assert!((audio.system_volume - 0.0).abs() < f32::EPSILON);
        audio.set_microphone_volume(2.0);
        assert!((audio.microphone_volume - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_audio_any_enabled() {
        let both = AudioSettings::new(true, true);
        assert!(both.any_audio_enabled());
        let none = AudioSettings::new(false, false);
        assert!(!none.any_audio_enabled());
        let sys_only = AudioSettings::new(true, false);
        assert!(sys_only.any_audio_enabled());
    }

    #[test]
    fn test_audio_volume_percent() {
        let audio = AudioSettings::default();
        assert_eq!(audio.system_volume_percent(), 80);
        assert_eq!(audio.microphone_volume_percent(), 70);
    }

    // -- CountdownTimer tests -----------------------------------------------

    #[test]
    fn test_countdown_new() {
        let timer = CountdownTimer::new(3);
        assert_eq!(timer.duration_secs, 3);
        assert_eq!(timer.remaining_secs, 3);
        assert!(!timer.active);
    }

    #[test]
    fn test_countdown_start_and_tick() {
        let mut timer = CountdownTimer::new(3);
        timer.start();
        assert!(timer.active);
        assert_eq!(timer.remaining_secs, 3);

        assert!(!timer.tick()); // 3 -> 2
        assert_eq!(timer.remaining_secs, 2);

        assert!(!timer.tick()); // 2 -> 1
        assert_eq!(timer.remaining_secs, 1);

        assert!(timer.tick()); // 1 -> 0 (finished!)
        assert_eq!(timer.remaining_secs, 0);
        assert!(!timer.active);
        assert!(timer.is_finished());
    }

    #[test]
    fn test_countdown_cancel() {
        let mut timer = CountdownTimer::new(3);
        timer.start();
        timer.tick();
        timer.cancel();
        assert!(!timer.active);
        assert_eq!(timer.remaining_secs, 3); // reset to duration
    }

    #[test]
    fn test_countdown_tick_when_inactive() {
        let mut timer = CountdownTimer::new(3);
        assert!(!timer.tick()); // not active, tick does nothing
        assert_eq!(timer.remaining_secs, 3);
    }

    #[test]
    fn test_countdown_set_duration() {
        let mut timer = CountdownTimer::new(3);
        timer.set_duration(5);
        assert_eq!(timer.duration_secs, 5);
        assert_eq!(timer.remaining_secs, 5);
    }

    #[test]
    fn test_countdown_reset() {
        let mut timer = CountdownTimer::new(3);
        timer.start();
        timer.tick();
        timer.reset();
        assert!(!timer.active);
        assert_eq!(timer.remaining_secs, 3);
    }

    // -- CursorOptions tests ------------------------------------------------

    #[test]
    fn test_cursor_options_default() {
        let opts = CursorOptions::default();
        assert!(opts.capture_cursor);
        assert!(opts.click_highlight);
        assert_eq!(opts.highlight_radius, 20);
    }

    #[test]
    fn test_cursor_options_new() {
        let opts = CursorOptions::new(false, false);
        assert!(!opts.capture_cursor);
        assert!(!opts.click_highlight);
    }

    // -- Annotation tests ---------------------------------------------------

    #[test]
    fn test_annotation_tool_labels() {
        assert_eq!(AnnotationTool::Rectangle.label(), "Rectangle");
        assert_eq!(AnnotationTool::Arrow.label(), "Arrow");
        assert_eq!(AnnotationTool::Text.label(), "Text");
        assert_eq!(AnnotationTool::Highlight.label(), "Highlight");
    }

    #[test]
    fn test_annotation_tool_all() {
        assert_eq!(AnnotationTool::all().len(), 4);
    }

    #[test]
    fn test_annotation_dimensions() {
        let mut ann = Annotation::new(AnnotationTool::Rectangle, 10.0, 20.0, colors::RED);
        ann.end_x = 110.0;
        ann.end_y = 70.0;
        assert!((ann.width() - 100.0).abs() < f32::EPSILON);
        assert!((ann.height() - 50.0).abs() < f32::EPSILON);
        assert!((ann.min_x() - 10.0).abs() < f32::EPSILON);
        assert!((ann.min_y() - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_annotation_render_rectangle() {
        let mut ann = Annotation::new(AnnotationTool::Rectangle, 0.0, 0.0, colors::RED);
        ann.end_x = 100.0;
        ann.end_y = 100.0;
        let cmds = ann.render();
        assert!(!cmds.is_empty());
        // Should contain a StrokeRect
        assert!(cmds.iter().any(|c| matches!(c, RenderCommand::StrokeRect { .. })));
    }

    #[test]
    fn test_annotation_render_arrow() {
        let mut ann = Annotation::new(AnnotationTool::Arrow, 0.0, 0.0, colors::BLUE);
        ann.end_x = 100.0;
        ann.end_y = 50.0;
        let cmds = ann.render();
        // Line + 2 arrowhead lines = 3 Line commands
        let line_count = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::Line { .. }))
            .count();
        assert_eq!(line_count, 3);
    }

    #[test]
    fn test_annotation_render_text() {
        let mut ann = Annotation::new(AnnotationTool::Text, 10.0, 10.0, colors::TEXT);
        ann.text_content = "Hello".to_string();
        let cmds = ann.render();
        assert!(cmds.iter().any(|c| matches!(c, RenderCommand::Text { .. })));
    }

    #[test]
    fn test_annotation_render_highlight() {
        let mut ann = Annotation::new(AnnotationTool::Highlight, 0.0, 0.0, Color::rgba(255, 255, 0, 80));
        ann.end_x = 50.0;
        ann.end_y = 50.0;
        let cmds = ann.render();
        assert!(cmds.iter().any(|c| matches!(c, RenderCommand::FillRect { .. })));
    }

    // -- OutputSettings tests -----------------------------------------------

    #[test]
    fn test_output_default() {
        let out = OutputSettings::default();
        assert_eq!(out.save_directory, PathBuf::from(DEFAULT_SAVE_DIR));
        assert_eq!(out.auto_increment, 1);
        assert_eq!(out.max_file_size, 0);
    }

    #[test]
    fn test_output_generate_filename() {
        let out = OutputSettings::default();
        let name = out.generate_filename(2026, 5, 18, 14, 30, 0);
        assert_eq!(name, "recording_20260518_143000");
    }

    #[test]
    fn test_output_generate_filename_with_increment() {
        let out = OutputSettings {
            filename_template: "rec_{n}_{date}".to_string(),
            ..OutputSettings::default()
        };
        let name = out.generate_filename(2026, 1, 1, 0, 0, 0);
        assert_eq!(name, "rec_0001_20260101");
    }

    #[test]
    fn test_output_generate_path() {
        let out = OutputSettings::default();
        let path = out.generate_path(2026, 5, 18, 12, 0, 0);
        assert!(path.to_string_lossy().contains("recording_20260518_120000"));
    }

    #[test]
    fn test_output_bump_increment() {
        let mut out = OutputSettings::default();
        assert_eq!(out.auto_increment, 1);
        out.bump_increment();
        assert_eq!(out.auto_increment, 2);
        out.bump_increment();
        assert_eq!(out.auto_increment, 3);
    }

    #[test]
    fn test_output_size_limit() {
        let mut out = OutputSettings::default();
        assert!(!out.has_size_limit());
        assert!(!out.exceeds_limit(999_999_999));

        out.max_file_size = 1_000_000;
        assert!(out.has_size_limit());
        assert!(!out.exceeds_limit(500_000));
        assert!(out.exceeds_limit(1_000_000));
        assert!(out.exceeds_limit(2_000_000));
    }

    #[test]
    fn test_output_max_size_display() {
        let mut out = OutputSettings::default();
        assert_eq!(out.max_size_display(), "No limit");

        out.max_file_size = 1_048_576;
        assert_eq!(out.max_size_display(), "1.0 MB");
    }

    // -- format helpers -----------------------------------------------------

    #[test]
    fn test_format_file_size() {
        assert_eq!(format_file_size(0), "0 B");
        assert_eq!(format_file_size(512), "512 B");
        assert_eq!(format_file_size(1024), "1.0 KB");
        assert_eq!(format_file_size(1_048_576), "1.0 MB");
        assert_eq!(format_file_size(1_073_741_824), "1.0 GB");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "00:00");
        assert_eq!(format_duration(65), "01:05");
        assert_eq!(format_duration(3661), "01:01:01");
    }

    // -- RecordingHistory tests ---------------------------------------------

    #[test]
    fn test_history_new_is_empty() {
        let h = RecordingHistory::new();
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
    }

    #[test]
    fn test_history_add_and_get() {
        let mut h = RecordingHistory::new();
        let entry = HistoryEntry::new(
            0,
            "test".to_string(),
            PathBuf::from("/tmp/test"),
            10,
            1000,
            300,
            30,
            1920,
            1080,
        );
        let id = h.add(entry);
        assert_eq!(h.len(), 1);
        let fetched = h.get(id).expect("should find entry");
        assert_eq!(fetched.name, "test");
    }

    #[test]
    fn test_history_remove() {
        let mut h = RecordingHistory::new();
        let entry = HistoryEntry::new(
            0,
            "test".to_string(),
            PathBuf::from("/tmp"),
            10,
            1000,
            300,
            30,
            1920,
            1080,
        );
        let id = h.add(entry);
        assert!(h.remove(id));
        assert!(h.is_empty());
        assert!(!h.remove(id)); // already removed
    }

    #[test]
    fn test_history_select() {
        let mut h = RecordingHistory::new();
        let e1 = HistoryEntry::new(0, "a".into(), PathBuf::new(), 10, 100, 30, 30, 1920, 1080);
        let e2 = HistoryEntry::new(0, "b".into(), PathBuf::new(), 20, 200, 60, 30, 1920, 1080);
        let id1 = h.add(e1);
        let _id2 = h.add(e2);

        h.select(id1);
        assert!(h.selected().is_some());
        assert_eq!(h.selected().expect("selected").name, "a");
    }

    #[test]
    fn test_history_total_size() {
        let mut h = RecordingHistory::new();
        h.add(HistoryEntry::new(0, "a".into(), PathBuf::new(), 10, 100, 30, 30, 1920, 1080));
        h.add(HistoryEntry::new(0, "b".into(), PathBuf::new(), 10, 200, 30, 30, 1920, 1080));
        assert_eq!(h.total_size(), 300);
    }

    #[test]
    fn test_history_clear() {
        let mut h = RecordingHistory::mock_entries();
        assert!(!h.is_empty());
        h.clear();
        assert!(h.is_empty());
    }

    #[test]
    fn test_history_entry_displays() {
        let e = HistoryEntry::new(1, "test".into(), PathBuf::new(), 3661, 1_048_576, 300, 30, 1920, 1080)
            .with_timestamp(2026, 5, 18, 14, 30, 0);
        assert_eq!(e.duration_display(), "01:01:01");
        assert_eq!(e.size_display(), "1.0 MB");
        assert_eq!(e.resolution_display(), "1920x1080");
        assert_eq!(e.timestamp_display(), "2026-05-18 14:30:00");
    }

    // -- TrimRange tests ----------------------------------------------------

    #[test]
    fn test_trim_new() {
        let trim = TrimRange::new(60.0);
        assert!((trim.start_secs - 0.0).abs() < f64::EPSILON);
        assert!((trim.end_secs - 60.0).abs() < f64::EPSILON);
        assert!(!trim.is_trimmed());
    }

    #[test]
    fn test_trim_set_start_end() {
        let mut trim = TrimRange::new(60.0);
        trim.set_start(10.0);
        trim.set_end(50.0);
        assert!((trim.start_secs - 10.0).abs() < f64::EPSILON);
        assert!((trim.end_secs - 50.0).abs() < f64::EPSILON);
        assert!(trim.is_trimmed());
        assert!((trim.trimmed_duration() - 40.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_trim_clamp_start() {
        let mut trim = TrimRange::new(60.0);
        trim.set_start(-5.0);
        assert!(trim.start_secs >= 0.0);
        trim.set_end(30.0);
        trim.set_start(35.0); // can't exceed end - 0.1
        assert!(trim.start_secs < trim.end_secs);
    }

    #[test]
    fn test_trim_clamp_end() {
        let mut trim = TrimRange::new(60.0);
        trim.set_end(100.0); // clamped to total
        assert!((trim.end_secs - 60.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_trim_fractions() {
        let mut trim = TrimRange::new(100.0);
        trim.set_start(25.0);
        trim.set_end(75.0);
        assert!((trim.start_fraction() - 0.25).abs() < 0.001);
        assert!((trim.end_fraction() - 0.75).abs() < 0.001);
    }

    #[test]
    fn test_trim_frame_count() {
        let mut trim = TrimRange::new(60.0);
        trim.set_start(10.0);
        trim.set_end(40.0);
        assert_eq!(trim.trimmed_frame_count(30), 900);
    }

    #[test]
    fn test_trim_reset() {
        let mut trim = TrimRange::new(60.0);
        trim.set_start(10.0);
        trim.set_end(50.0);
        trim.reset();
        assert!(!trim.is_trimmed());
    }

    // -- HotkeyBinding tests ------------------------------------------------

    #[test]
    fn test_hotkey_display_simple() {
        let hk = HotkeyBinding::new(HotkeyAction::StartStop, "F9", false, false, false);
        assert_eq!(hk.display(), "F9");
    }

    #[test]
    fn test_hotkey_display_modifiers() {
        let hk = HotkeyBinding::new(HotkeyAction::Screenshot, "S", true, true, false);
        assert_eq!(hk.display(), "Ctrl+Shift+S");
    }

    #[test]
    fn test_hotkey_display_all_modifiers() {
        let hk = HotkeyBinding::new(HotkeyAction::Cancel, "Escape", true, true, true);
        assert_eq!(hk.display(), "Ctrl+Shift+Alt+Escape");
    }

    #[test]
    fn test_hotkey_action_labels() {
        assert_eq!(HotkeyAction::StartStop.label(), "Start / Stop");
        assert_eq!(HotkeyAction::PauseResume.label(), "Pause / Resume");
        assert_eq!(HotkeyAction::Screenshot.label(), "Screenshot");
        assert_eq!(HotkeyAction::Cancel.label(), "Cancel");
    }

    #[test]
    fn test_default_hotkeys() {
        let hks = default_hotkeys();
        assert_eq!(hks.len(), 4);
        assert!(hks.iter().any(|h| h.action == HotkeyAction::StartStop));
        assert!(hks.iter().any(|h| h.action == HotkeyAction::PauseResume));
    }

    // -- ScheduledRecording tests -------------------------------------------

    #[test]
    fn test_schedule_new() {
        let s = ScheduledRecording::new(1, 14, 30, 300, "Meeting record");
        assert_eq!(s.start_hour, 14);
        assert_eq!(s.start_minute, 30);
        assert_eq!(s.duration_secs, 300);
        assert!(s.enabled);
        assert!(s.one_shot);
    }

    #[test]
    fn test_schedule_time_clamping() {
        let s = ScheduledRecording::new(1, 25, 61, 0, "test");
        assert_eq!(s.start_hour, 23);
        assert_eq!(s.start_minute, 59);
    }

    #[test]
    fn test_schedule_display() {
        let s = ScheduledRecording::new(1, 9, 5, 0, "test");
        assert_eq!(s.start_time_display(), "09:05");
        assert_eq!(s.duration_display(), "Until stopped");

        let s2 = ScheduledRecording::new(1, 14, 30, 3600, "test");
        assert_eq!(s2.duration_display(), "01:00:00");
    }

    #[test]
    fn test_schedule_trigger() {
        let s = ScheduledRecording::new(1, 14, 30, 300, "test");
        assert!(s.should_trigger(14, 30));
        assert!(!s.should_trigger(14, 31));
        assert!(!s.should_trigger(15, 30));
    }

    #[test]
    fn test_schedule_trigger_disabled() {
        let mut s = ScheduledRecording::new(1, 14, 30, 300, "test");
        s.enabled = false;
        assert!(!s.should_trigger(14, 30));
    }

    #[test]
    fn test_schedule_mark_triggered_one_shot() {
        let mut s = ScheduledRecording::new(1, 14, 30, 300, "test");
        s.mark_triggered();
        assert!(!s.enabled);
    }

    #[test]
    fn test_schedule_mark_triggered_repeating() {
        let mut s = ScheduledRecording::new(1, 14, 30, 300, "test");
        s.one_shot = false;
        s.mark_triggered();
        assert!(s.enabled); // stays enabled
    }

    // -- ScheduleManager tests ----------------------------------------------

    #[test]
    fn test_schedule_manager_add_remove() {
        let mut mgr = ScheduleManager::new();
        assert!(mgr.is_empty());

        let s = ScheduledRecording::new(0, 10, 0, 600, "test");
        let id = mgr.add(s);
        assert_eq!(mgr.len(), 1);
        assert!(mgr.get(id).is_some());

        assert!(mgr.remove(id));
        assert!(mgr.is_empty());
    }

    #[test]
    fn test_schedule_manager_check_triggers() {
        let mut mgr = ScheduleManager::new();
        mgr.add(ScheduledRecording::new(0, 10, 0, 600, "a"));
        mgr.add(ScheduledRecording::new(0, 10, 0, 300, "b"));
        mgr.add(ScheduledRecording::new(0, 11, 0, 300, "c"));

        let triggers = mgr.check_triggers(10, 0);
        assert_eq!(triggers.len(), 2);

        let triggers = mgr.check_triggers(11, 0);
        assert_eq!(triggers.len(), 1);
    }

    #[test]
    fn test_schedule_manager_active_count() {
        let mut mgr = ScheduleManager::new();
        mgr.add(ScheduledRecording::new(0, 10, 0, 600, "a"));
        let mut s = ScheduledRecording::new(0, 11, 0, 300, "b");
        s.enabled = false;
        mgr.add(s);

        assert_eq!(mgr.active_count(), 1);
    }

    // -- RegionSelector tests -----------------------------------------------

    #[test]
    fn test_region_selector_new() {
        let rs = RegionSelector::new(1920.0, 1080.0);
        assert!(!rs.active);
        assert!(!rs.dragging);
    }

    #[test]
    fn test_region_selector_drag() {
        let mut rs = RegionSelector::new(1920.0, 1080.0);
        rs.activate();
        assert!(rs.active);

        rs.begin_drag(100.0, 100.0);
        assert!(rs.dragging);

        rs.update_drag(300.0, 250.0);
        let (x, y, w, h) = rs.current_rect();
        assert!((x - 100.0).abs() < f32::EPSILON);
        assert!((y - 100.0).abs() < f32::EPSILON);
        assert!((w - 200.0).abs() < f32::EPSILON);
        assert!((h - 150.0).abs() < f32::EPSILON);

        let region = rs.end_drag();
        assert!(region.is_some());
        let (rx, ry, rw, rh) = region.unwrap();
        assert!((rx - 100.0).abs() < f32::EPSILON);
        assert!((ry - 100.0).abs() < f32::EPSILON);
        assert!((rw - 200.0).abs() < f32::EPSILON);
        assert!((rh - 150.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_region_selector_too_small() {
        let mut rs = RegionSelector::new(1920.0, 1080.0);
        rs.activate();
        rs.begin_drag(100.0, 100.0);
        rs.update_drag(105.0, 105.0); // only 5x5, too small
        let region = rs.end_drag();
        assert!(region.is_none());
    }

    #[test]
    fn test_region_selector_cancel() {
        let mut rs = RegionSelector::new(1920.0, 1080.0);
        rs.activate();
        rs.begin_drag(100.0, 100.0);
        rs.cancel();
        assert!(!rs.active);
        assert!(!rs.dragging);
    }

    #[test]
    fn test_region_selector_render_inactive() {
        let rs = RegionSelector::new(1920.0, 1080.0);
        let cmds = rs.render();
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_region_selector_render_active() {
        let mut rs = RegionSelector::new(1920.0, 1080.0);
        rs.activate();
        rs.begin_drag(100.0, 100.0);
        rs.update_drag(300.0, 300.0);
        let cmds = rs.render();
        assert!(!cmds.is_empty());
    }

    // -- BMP encoder tests --------------------------------------------------

    #[test]
    fn test_bmp_encode_1x1() {
        let pixels = vec![0xFF_FF_00_00_u32]; // ARGB red
        let result = encode_bmp_frame(1, 1, &pixels);
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(&data[0..2], b"BM");
        // 14 + 40 + 4 = 58 bytes total for 1x1 BMP
        assert_eq!(data.len(), 58);
    }

    #[test]
    fn test_bmp_encode_pixel_mismatch() {
        let pixels = vec![0u32; 5]; // 5 pixels for 2x3 = wrong
        let result = encode_bmp_frame(2, 3, &pixels);
        assert!(result.is_err());
    }

    #[test]
    fn test_bmp_encode_2x2() {
        let pixels = vec![0xFF_00_00_00_u32; 4]; // 2x2 black
        let result = encode_bmp_frame(2, 2, &pixels);
        assert!(result.is_ok());
        let data = result.unwrap();
        // 14 + 40 + (2*4*2) = 70
        assert_eq!(data.len(), 70);
    }

    // -- RecordingIndicator tests -------------------------------------------

    #[test]
    fn test_indicator_new() {
        let ind = RecordingIndicator::new(10.0, 10.0);
        assert!(ind.visible);
        assert_eq!(ind.elapsed_secs, 0);
        assert_eq!(ind.file_size, 0);
    }

    #[test]
    fn test_indicator_update() {
        let mut ind = RecordingIndicator::new(10.0, 10.0);
        ind.update(30, 1_000_000, 30);
        assert_eq!(ind.elapsed_secs, 30);
        assert_eq!(ind.file_size, 1_000_000);
        assert_eq!(ind.current_fps, 30);
    }

    #[test]
    fn test_indicator_toggle_blink() {
        let mut ind = RecordingIndicator::new(10.0, 10.0);
        assert!(ind.blink_on);
        ind.toggle_blink();
        assert!(!ind.blink_on);
        ind.toggle_blink();
        assert!(ind.blink_on);
    }

    #[test]
    fn test_indicator_render_visible() {
        let ind = RecordingIndicator::new(10.0, 10.0);
        let cmds = ind.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_indicator_render_hidden() {
        let mut ind = RecordingIndicator::new(10.0, 10.0);
        ind.visible = false;
        let cmds = ind.render();
        assert!(cmds.is_empty());
    }

    // -- App state machine tests --------------------------------------------

    #[test]
    fn test_app_new_defaults() {
        let app = ScreenRecorderApp::new();
        assert_eq!(app.recording_state, RecordingState::Idle);
        assert_eq!(app.capture_mode, CaptureMode::FullScreen);
        assert_eq!(app.fps_preset, FpsPreset::Fps30);
        assert_eq!(app.total_frames, 0);
        assert_eq!(app.total_bytes, 0);
        assert_eq!(app.active_view, ActiveView::Record);
    }

    #[test]
    fn test_app_start_recording_with_countdown() {
        let mut app = ScreenRecorderApp::new();
        app.start_recording();
        assert_eq!(app.recording_state, RecordingState::Countdown);
        assert!(app.countdown.active);
    }

    #[test]
    fn test_app_start_recording_no_countdown() {
        let mut app = ScreenRecorderApp::new();
        app.countdown.set_duration(0);
        app.start_recording();
        assert_eq!(app.recording_state, RecordingState::Recording);
    }

    #[test]
    fn test_app_stop_recording() {
        let mut app = ScreenRecorderApp::new();
        app.countdown.set_duration(0);
        app.start_recording();
        app.stop_recording();
        assert_eq!(app.recording_state, RecordingState::Stopped);
    }

    #[test]
    fn test_app_pause_resume() {
        let mut app = ScreenRecorderApp::new();
        app.countdown.set_duration(0);
        app.start_recording();
        app.pause_recording();
        assert_eq!(app.recording_state, RecordingState::Paused);
        app.resume_recording();
        assert_eq!(app.recording_state, RecordingState::Recording);
    }

    #[test]
    fn test_app_countdown_to_recording() {
        let mut app = ScreenRecorderApp::new();
        app.start_recording(); // starts countdown (3s)
        assert_eq!(app.recording_state, RecordingState::Countdown);

        assert!(!app.tick_countdown()); // 3 -> 2
        assert!(!app.tick_countdown()); // 2 -> 1
        assert!(app.tick_countdown());  // 1 -> 0, recording starts
        assert_eq!(app.recording_state, RecordingState::Recording);
    }

    #[test]
    fn test_app_record_frame() {
        let mut app = ScreenRecorderApp::new();
        app.countdown.set_duration(0);
        app.start_recording();
        let frame = app.record_frame(8294400); // 1920*1080*4
        assert_eq!(frame, 1);
        assert_eq!(app.total_bytes, 8294400);
    }

    #[test]
    fn test_app_tick_elapsed() {
        let mut app = ScreenRecorderApp::new();
        app.countdown.set_duration(0);
        app.start_recording();
        app.tick_elapsed();
        app.tick_elapsed();
        assert_eq!(app.elapsed_secs, 2);
    }

    #[test]
    fn test_app_save_to_history() {
        let mut app = ScreenRecorderApp::new();
        let initial_count = app.history.len();
        app.save_to_history();
        assert_eq!(app.history.len(), initial_count + 1);
        assert_eq!(app.output.auto_increment, 2); // bumped
    }

    #[test]
    fn test_app_reset() {
        let mut app = ScreenRecorderApp::new();
        app.countdown.set_duration(0);
        app.start_recording();
        app.record_frame(1000);
        app.tick_elapsed();
        app.reset();
        assert_eq!(app.recording_state, RecordingState::Idle);
        assert_eq!(app.total_frames, 0);
        assert_eq!(app.total_bytes, 0);
        assert_eq!(app.elapsed_secs, 0);
    }

    #[test]
    fn test_app_setup_trim() {
        let mut app = ScreenRecorderApp::new();
        // No selection => no trim
        assert!(!app.setup_trim_for_selected());

        // Select first entry and try again
        if let Some(entry) = app.history.entries.first() {
            let id = entry.id;
            app.history.select(id);
            assert!(app.setup_trim_for_selected());
            assert!(app.trim.is_some());
            assert_eq!(app.active_view, ActiveView::Trim);
        }
    }

    // -- Render output tests ------------------------------------------------

    #[test]
    fn test_render_produces_commands() {
        let app = ScreenRecorderApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
        // Should start with the background FillRect
        assert!(matches!(cmds.first(), Some(RenderCommand::FillRect { .. })));
    }

    #[test]
    fn test_render_contains_sidebar() {
        let app = ScreenRecorderApp::new();
        let cmds = app.render();
        // Should contain text "Screen Recorder" somewhere
        let has_title = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "Screen Recorder"
            } else {
                false
            }
        });
        assert!(has_title);
    }

    #[test]
    fn test_render_record_view() {
        let app = ScreenRecorderApp::new();
        let cmds = app.render();
        let has_preview = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "Preview"
            } else {
                false
            }
        });
        assert!(has_preview);
    }

    #[test]
    fn test_render_history_view() {
        let mut app = ScreenRecorderApp::new();
        app.active_view = ActiveView::History;
        let cmds = app.render();
        let has_history_title = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "Recording History"
            } else {
                false
            }
        });
        assert!(has_history_title);
    }

    #[test]
    fn test_render_settings_view() {
        let mut app = ScreenRecorderApp::new();
        app.active_view = ActiveView::Settings;
        let cmds = app.render();
        let has_output = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "Output"
            } else {
                false
            }
        });
        assert!(has_output);
    }

    #[test]
    fn test_render_trim_view_no_selection() {
        let mut app = ScreenRecorderApp::new();
        app.active_view = ActiveView::Trim;
        let cmds = app.render();
        let has_msg = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("Select a recording")
            } else {
                false
            }
        });
        assert!(has_msg);
    }

    #[test]
    fn test_render_trim_view_with_selection() {
        let mut app = ScreenRecorderApp::new();
        app.trim = Some(TrimRange::new(60.0));
        app.active_view = ActiveView::Trim;
        let cmds = app.render();
        let has_apply = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "Apply Trim"
            } else {
                false
            }
        });
        assert!(has_apply);
    }

    #[test]
    fn test_render_countdown_overlay() {
        let mut app = ScreenRecorderApp::new();
        app.start_recording(); // triggers countdown
        let cmds = app.render();
        let has_get_ready = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("Get ready")
            } else {
                false
            }
        });
        assert!(has_get_ready);
    }

    #[test]
    fn test_render_recording_indicator() {
        let mut app = ScreenRecorderApp::new();
        app.countdown.set_duration(0);
        app.start_recording();
        app.indicator.update(10, 50000, 30);
        let cmds = app.render();
        // Should have the indicator overlay with time/fps
        let has_fps = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("fps")
            } else {
                false
            }
        });
        assert!(has_fps);
    }

    #[test]
    fn test_render_settings_hotkeys_tab() {
        let mut app = ScreenRecorderApp::new();
        app.active_view = ActiveView::Settings;
        app.settings_tab = SettingsTab::Hotkeys;
        let cmds = app.render();
        let has_f9 = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "F9"
            } else {
                false
            }
        });
        assert!(has_f9);
    }

    #[test]
    fn test_render_settings_schedule_tab_empty() {
        let mut app = ScreenRecorderApp::new();
        app.active_view = ActiveView::Settings;
        app.settings_tab = SettingsTab::Schedule;
        let cmds = app.render();
        let has_add = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "+ Add Schedule"
            } else {
                false
            }
        });
        assert!(has_add);
    }

    #[test]
    fn test_render_settings_schedule_tab_with_entries() {
        let mut app = ScreenRecorderApp::new();
        app.active_view = ActiveView::Settings;
        app.settings_tab = SettingsTab::Schedule;
        app.schedules.add(ScheduledRecording::new(0, 14, 0, 300, "Daily standup"));
        let cmds = app.render();
        let has_label = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "Daily standup"
            } else {
                false
            }
        });
        assert!(has_label);
    }

    #[test]
    fn test_render_annotation_toolbar() {
        let mut app = ScreenRecorderApp::new();
        app.countdown.set_duration(0);
        app.start_recording();
        app.annotation_toolbar_visible = true;
        let cmds = app.render();
        let has_rect_tool = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "Rectangle"
            } else {
                false
            }
        });
        assert!(has_rect_tool);
    }

    #[test]
    fn test_render_status_bar_idle() {
        let app = ScreenRecorderApp::new();
        let cmds = app.render();
        let has_ready = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "Ready to record"
            } else {
                false
            }
        });
        assert!(has_ready);
    }

    #[test]
    fn test_render_status_bar_recording() {
        let mut app = ScreenRecorderApp::new();
        app.countdown.set_duration(0);
        app.start_recording();
        app.total_frames = 100;
        app.total_bytes = 50000;
        app.elapsed_secs = 5;
        let cmds = app.render();
        let has_stats = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("100 frames")
            } else {
                false
            }
        });
        assert!(has_stats);
    }
}
