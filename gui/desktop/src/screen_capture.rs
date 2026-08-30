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
//!
//! # Colour
//!
//! Both renderers here read every colour from the [`Palette`] the caller
//! supplies; this module holds none of its own. Four judgements decide which
//! role goes where, and each is a test rather than a comment.
//!
//! 1. **The indicator pill is as transparent as the user asked.** It used to
//!    be `Color::rgba(MOCHA_BASE.r, MOCHA_BASE.g, MOCHA_BASE.b, 220)` — the
//!    base colour with an alpha soldered onto it, which is not a colour
//!    someone forgot to theme but the *transparency setting*, frozen. A user
//!    who had turned transparency off still saw their desktop through the
//!    recording badge; one who turned it up to Full got a badge more solid
//!    than every other floating surface on the screen.
//!    [`Palette::panel_bg`] is `base` at the palette's own `panel_alpha`, so
//!    the pill now answers the setting.
//! 2. **A button's lettering is computed from its own fill, and this is the
//!    one conversion here that fixes a visible bug rather than a latent
//!    one.** Record, Pause and Resume were lettered with the near-black
//!    `MOCHA_BASE`, which is legible on Mocha's pale red, yellow and green —
//!    and illegible on the light theme's, where all three fills are *dark*
//!    enough that [`readable_on`](appearance::readable_on) answers with the
//!    pale endpoint instead. Naming the ink beside the fill is what allowed the
//!    two to disagree; [`readable_on`](appearance::readable_on) of the fill
//!    cannot. The tests pin the endpoint each mode must produce, computed by
//!    hand rather than by calling the same function the renderer calls.
//! 3. **The transport colours are a code, not decoration, so nothing here is
//!    accented.** Red records, yellow pauses, green resumes, and grey means
//!    neither — a user reads those hues as meaning, the way they read a
//!    traffic light, so they keep their named roles across a retheme instead
//!    of following the accent. The accent says what is *in force*; on this
//!    panel the state already says that, in a colour the user has learned.
//!    The count of accent-coloured commands is asserted at zero so that an
//!    accent appearing here later has to be a decision rather than a slip.
//! 4. **The indicator is silent unless something is happening**, which is a
//!    fact about coverage as much as about rendering. Four of the six
//!    [`RecordingState`]s draw no indicator at all, and three of them reach
//!    only the controls panel's fallback arm. A colour test that rendered
//!    "the recorder" without naming its state would therefore be testing two
//!    states out of six and reporting on all of them, so every test below
//!    iterates the states explicitly.

use appearance::{Palette, readable_on};
use guitk::idseq::IdSeq;
use guitk::ratio;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

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
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Total pixel count. Cannot overflow: two `u32` dimensions widened to
    /// `u64` multiply to at most 2^64, and a capture region is bounded by the
    /// display long before that.
    #[must_use]
    pub fn pixel_count(&self) -> u64 {
        u64::from(self.width).saturating_mul(u64::from(self.height))
    }

    /// Estimate raw frame size in bytes (BGRA = 4 bytes per pixel).
    #[must_use]
    pub fn frame_size_bytes(&self) -> u64 {
        self.pixel_count().saturating_mul(4)
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

    /// Elapsed recording time, `mm:ss` widening to `hh:mm:ss` past an hour.
    ///
    /// Shares [`guitk::duration::clock`] with [`RecordingEntry::duration_display`]
    /// because the two render the *same integer*: `ScreenRecorder::stop` writes
    /// `duration_ms: self.stats.elapsed_ms`. They used to be two functions and
    /// disagreed past an hour — see that method's note.
    pub fn elapsed_display(&self) -> String {
        guitk::duration::clock(self.elapsed_ms / 1000)
    }

    /// Average fps achieved.
    pub fn average_fps(&self) -> f64 {
        if self.elapsed_ms == 0 {
            return 0.0;
        }
        (self.frames_captured as f64 * 1000.0) / self.elapsed_ms as f64
    }

    /// Drop rate as a percentage of the frames the recorder saw at all.
    #[must_use]
    pub fn drop_rate_pct(&self) -> f64 {
        let total = self.frames_captured.saturating_add(self.dropped_frames);
        ratio::percent(self.dropped_frames, total).unwrap_or(0.0)
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
    guitk::bytes::iec(bytes)
}

// ============================================================================
// Recording history
// ============================================================================

/// Identifier for a completed recording.
///
/// 64 bits rather than 32 so that the sequence handing these out cannot run
/// out — see [`guitk::idseq`].
pub type RecordingId = u64;

/// A completed recording in history.
#[derive(Clone, Debug)]
pub struct RecordingEntry {
    pub id: RecordingId,
    pub filename: String,
    pub timestamp: u64,
    pub duration_ms: u64,
    pub file_size: u64,
    pub region: CaptureRegion,
    pub frame_rate: FrameRate,
    pub format: OutputFormat,
}

impl RecordingEntry {
    /// The finished recording's length, in the same shape the overlay showed
    /// while it was being made.
    ///
    /// `duration_ms` is not merely *like* `RecordingStats::elapsed_ms` — it is
    /// assigned from it, unmodified, in `ScreenRecorder::stop`. This method
    /// nevertheless used to compute `mins = total_secs / 60` with no hour
    /// branch, so a recording of one hour and one minute read `01:01:01` in the
    /// overlay and `61:01` here, a moment later, for one integer. Both had
    /// tests, at 3 661 000 ms and 125 000 ms respectively, so neither ever
    /// evaluated the other's input.
    pub fn duration_display(&self) -> String {
        guitk::duration::clock(self.duration_ms / 1000)
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
    ids: IdSeq<RecordingId>,
}

impl ScreenRecorder {
    pub fn new() -> Self {
        Self {
            config: RecordingConfig::default_config(),
            state: RecordingState::Idle,
            stats: RecordingStats::new(),
            history: Vec::new(),
            ids: IdSeq::new(),
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
                // Saturating so that the countdown's floor is a property of
                // this expression rather than of the branch above it.
                remaining_secs: remaining_secs.saturating_sub(1),
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
            id: self.ids.issue_infallible(),
            filename: filename.to_string(),
            timestamp,
            duration_ms: self.stats.elapsed_ms,
            file_size,
            region: self.config.effective_region(),
            frame_rate: self.config.frame_rate,
            format: self.config.output_format,
        };
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

    /// Check if max duration reached. A limit of zero is no limit.
    pub fn is_duration_exceeded(&self) -> bool {
        self.config.max_duration_secs > 0
            && self.stats.elapsed_ms
                >= u64::from(self.config.max_duration_secs).saturating_mul(1000)
    }

    /// Delete a history entry by ID.
    pub fn delete_history(&mut self, id: RecordingId) -> bool {
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
///
/// `p` supplies every colour drawn; see this module's `# Colour` section for
/// the judgements behind the choices.
///
/// Returns no commands at all unless the recorder is active or processing —
/// judgement 4, and the reason a caller must not treat an empty vector as an
/// error.
pub fn render_recording_indicator(
    recorder: &ScreenRecorder,
    p: &Palette,
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
        x,
        y,
        width: w,
        height: h,
        // Judgement 1: the transparency setting, not a baked-in alpha.
        color: p.panel_bg(),
        corner_radii: CornerRadii::all(16.0),
    });

    // Recording dot (pulsing red when recording, yellow when paused).
    //
    // Judgement 3: these three are a code the user reads, not a theme — so
    // they keep their named roles rather than following the accent.
    let dot_color = match recorder.state {
        RecordingState::Recording => p.red,
        RecordingState::Paused => p.yellow,
        _ => p.overlay0,
    };
    cmds.push(RenderCommand::FillRect {
        x: x + 10.0,
        y: y + 10.0,
        width: 12.0,
        height: 12.0,
        color: dot_color,
        corner_radii: CornerRadii::all(6.0),
    });

    // Time display.
    cmds.push(RenderCommand::Text {
        x: x + 28.0,
        y: y + 8.0,
        text: recorder.stats.elapsed_display(),
        font_size: 13.0,
        color: p.text,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // State label.
    let state_label = recorder.state.label();
    cmds.push(RenderCommand::Text {
        x: x + 90.0,
        y: y + 10.0,
        text: state_label.to_string(),
        font_size: 10.0,
        // Quieter than the timer beside it: the clock is the reading, the
        // word is a label on it.
        color: p.subtext0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    cmds
}

/// Render the recording toolbar/control panel.
///
/// `p` supplies every colour drawn; see this module's `# Colour` section.
pub fn render_recording_controls(
    recorder: &ScreenRecorder,
    p: &Palette,
    x: f32,
    y: f32,
    w: f32,
) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();
    let h = 60.0;

    // Background.
    cmds.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        // A rung *below* the base this panel sits on, so it reads as a bar
        // laid on the desktop rather than a sheet of it.
        color: p.mantle,
        corner_radii: CornerRadii::all(8.0),
    });

    // Title.
    cmds.push(RenderCommand::Text {
        x: x + 12.0,
        y: y + 6.0,
        text: "Screen Recorder".to_string(),
        font_size: 13.0,
        color: p.text,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    let btn_y = y + 28.0;
    let btn_h = 24.0;

    match recorder.state {
        RecordingState::Idle => {
            // Record button.
            cmds.push(RenderCommand::FillRect {
                x: x + 12.0,
                y: btn_y,
                width: 80.0,
                height: btn_h,
                color: p.red,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 26.0,
                y: btn_y + 5.0,
                text: "Record".to_string(),
                font_size: 12.0,
                // Judgement 2: read off the fill. Naming a near-black here is
                // what made this button unreadable on a light theme.
                color: readable_on(p.red),
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
        RecordingState::Recording => {
            // Pause button.
            cmds.push(RenderCommand::FillRect {
                x: x + 12.0,
                y: btn_y,
                width: 70.0,
                height: btn_h,
                color: p.yellow,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 24.0,
                y: btn_y + 5.0,
                text: "Pause".to_string(),
                font_size: 12.0,
                // Judgement 2.
                color: readable_on(p.yellow),
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Stop button.
            cmds.push(RenderCommand::FillRect {
                x: x + 92.0,
                y: btn_y,
                width: 60.0,
                height: btn_h,
                // Not part of the colour code: Stop is available in both
                // running states and means the same thing in each, so it is
                // a plain surface rung rather than a fourth hue.
                color: p.surface1,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 106.0,
                y: btn_y + 5.0,
                text: "Stop".to_string(),
                font_size: 12.0,
                // A surface rung takes ordinary body ink; only the three
                // strong-hue buttons need judgement 2's computation.
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
        RecordingState::Paused => {
            // Resume button.
            cmds.push(RenderCommand::FillRect {
                x: x + 12.0,
                y: btn_y,
                width: 80.0,
                height: btn_h,
                color: p.green,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 22.0,
                y: btn_y + 5.0,
                text: "Resume".to_string(),
                font_size: 12.0,
                // Judgement 2.
                color: readable_on(p.green),
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Stop button.
            cmds.push(RenderCommand::FillRect {
                x: x + 102.0,
                y: btn_y,
                width: 60.0,
                height: btn_h,
                // As in the Recording arm: the same button, the same rung.
                color: p.surface1,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 116.0,
                y: btn_y + 5.0,
                text: "Stop".to_string(),
                font_size: 12.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
        _ => {
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: btn_y + 4.0,
                text: recorder.state.label().to_string(),
                font_size: 12.0,
                // The three transient states — counting down, choosing a
                // region, encoding — offer no button, so the word replaces
                // one. Peach because it is the shell's "working on it" hue
                // and none of red/yellow/green is free to mean it here.
                color: p.peach,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }

    // Stats (right side).
    if recorder.state.is_active() || recorder.state == RecordingState::Processing {
        let stats_x = x + w - 200.0;
        cmds.push(RenderCommand::Text {
            x: stats_x,
            y: y + 8.0,
            text: format!(
                "Frames: {} ({:.1} fps)",
                recorder.stats.frames_captured,
                recorder.stats.average_fps(),
            ),
            font_size: 10.0,
            // Telemetry, quieter than the title it sits beside.
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cmds.push(RenderCommand::Text {
            x: stats_x,
            y: y + 22.0,
            text: format!(
                "Size: {} | Drops: {:.1}%",
                recorder.stats.size_display(),
                recorder.stats.drop_rate_pct(),
            ),
            font_size: 10.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cmds.push(RenderCommand::Text {
            x: stats_x,
            y: y + 36.0,
            text: format!("Time: {}", recorder.stats.elapsed_display()),
            font_size: 10.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    cmds
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]
    // These tests assert a float equals the exact literal the code under test was
    // handed. That is the assertion meant: a tolerance would let a value that has
    // drifted pass as one that has not.
    #![allow(clippy::float_cmp)]

    use super::*;
    use crate::palette_check::assert_drawn_from;
    use guitk::color::Color;

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
        assert_eq!(s.size_display(), "2.0 KiB");
        s.bytes_written = 5 * 1024 * 1024;
        assert_eq!(s.size_display(), "5.0 MiB");
    }

    // --- RecordingEntry ---
    #[test]
    fn test_entry_duration_display() {
        let entry = RecordingEntry {
            id: 1,
            filename: "test.mp4".to_string(),
            timestamp: 0,
            duration_ms: 125000,
            file_size: 1024,
            region: CaptureRegion::new(0, 0, 800, 600),
            frame_rate: FrameRate::Fps30,
            format: OutputFormat::Mp4,
        };
        // "02:05", not "2:05": the field is zero-padded because the overlay
        // that showed this same recording being made pads it, and the list is
        // read moments after the overlay disappears.
        assert_eq!(entry.duration_display(), "02:05");
    }

    /// `ScreenRecorder::stop` writes `duration_ms: self.stats.elapsed_ms`, so
    /// these two methods render one integer. They used to be two independent
    /// formatters and disagreed past an hour — the overlay read `01:01:01`
    /// and the list `61:01` — because each was tested only at a value that
    /// hid the disagreement (3 661 000 ms here, 125 000 ms there).
    #[test]
    fn test_overlay_and_list_agree_for_the_same_recording() {
        for elapsed_ms in [0_u64, 125_000, 3_600_000, 3_661_000, 90_061_000] {
            let mut stats = RecordingStats::new();
            stats.elapsed_ms = elapsed_ms;
            let entry = RecordingEntry {
                id: 1,
                filename: "test.mp4".to_string(),
                timestamp: 0,
                duration_ms: elapsed_ms,
                file_size: 1024,
                region: CaptureRegion::new(0, 0, 800, 600),
                frame_rate: FrameRate::Fps30,
                format: OutputFormat::Mp4,
            };
            assert_eq!(
                stats.elapsed_display(),
                entry.duration_display(),
                "{elapsed_ms} ms rendered two ways"
            );
        }
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
        assert!(matches!(
            r.state,
            RecordingState::Countdown { remaining_secs: 3 }
        ));
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
        r.start();
        r.stop();
        r.finish_processing("a.mp4", 1, 0);
        r.start();
        r.stop();
        r.finish_processing("b.mp4", 2, 1);
        assert_eq!(r.history.len(), 2);
        r.clear_history();
        assert!(r.history.is_empty());
    }

    // --- Rendering ---
    #[test]
    fn test_indicator_idle_empty() {
        let r = ScreenRecorder::new();
        let cmds = render_recording_indicator(&r, &accented(false), 0.0, 0.0);
        assert!(cmds.is_empty()); // No indicator when idle
    }

    #[test]
    fn test_indicator_recording() {
        let mut r = ScreenRecorder::new();
        r.config.countdown_secs = 0;
        r.start();
        let cmds = render_recording_indicator(&r, &accented(false), 10.0, 10.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_controls_idle() {
        let r = ScreenRecorder::new();
        let cmds = render_recording_controls(&r, &accented(false), 0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_controls_recording() {
        let mut r = ScreenRecorder::new();
        r.config.countdown_secs = 0;
        r.start();
        let cmds = render_recording_controls(&r, &accented(false), 0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_controls_paused() {
        let mut r = ScreenRecorder::new();
        r.config.countdown_secs = 0;
        r.start();
        r.pause();
        let cmds = render_recording_controls(&r, &accented(false), 0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    // ------------------------------------------------------------------
    // Colour
    //
    // Part 2 of TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-
    // PALETTE. Ten constants were deleted from this module; these tests are
    // what makes their deletion checkable rather than merely plausible.
    // ------------------------------------------------------------------

    /// A palette whose accent is in neither theme's role list.
    ///
    /// The shipped accent *is* `blue`, so an "is anything accented?" count run
    /// against a stock palette cannot distinguish an accent-coloured command
    /// from a legitimately blue one, and would pass whatever the renderer did.
    /// Magenta is off both palettes, which is asserted here rather than
    /// assumed — a future palette that adopted it would silently turn
    /// `nothing_in_the_recorder_is_accented` into a tautology.
    fn accented(light: bool) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = Color::from_hex(0xFF00FF);
        for (name, role) in p.roles() {
            if name == "accent" {
                continue;
            }
            assert!(
                !(role.r == p.accent.r && role.g == p.accent.g && role.b == p.accent.b),
                "the fixture accent collides with role `{name}`, so an accent \
                 assertion in this module proves nothing"
            );
        }
        p
    }

    /// Every state the recorder can be in, with a name for failure messages.
    ///
    /// Judgement 4: four of these six draw no indicator and three reach only
    /// the controls panel's fallback arm, so a test that rendered "the
    /// recorder" without naming a state would be exercising two of six and
    /// reporting on all of them.
    fn all_states() -> [(RecordingState, &'static str); 6] {
        [
            (RecordingState::Idle, "Idle"),
            (RecordingState::Countdown { remaining_secs: 2 }, "Countdown"),
            (RecordingState::SelectingRegion, "SelectingRegion"),
            (RecordingState::Recording, "Recording"),
            (RecordingState::Paused, "Paused"),
            (RecordingState::Processing, "Processing"),
        ]
    }

    /// A recorder parked in `state`, with stats a rendering can format.
    fn recorder_in(state: RecordingState) -> ScreenRecorder {
        let mut r = ScreenRecorder::new();
        r.state = state;
        r.stats.frames_captured = 120;
        r.stats.elapsed_ms = 4_000;
        r.stats.bytes_written = 1_048_576;
        r
    }

    /// The colour a command puts on the screen, if it puts one there.
    fn color_of(cmd: &RenderCommand) -> Option<Color> {
        match cmd {
            RenderCommand::FillRect { color, .. }
            | RenderCommand::StrokeRect { color, .. }
            | RenderCommand::Text { color, .. }
            | RenderCommand::Line { color, .. }
            | RenderCommand::BoxShadow { color, .. } => Some(*color),
            _ => None,
        }
    }

    /// Every colour in `cmds`, in draw order.
    fn colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter().filter_map(color_of).collect()
    }

    /// The colour of the one text command reading `s`.
    fn text_color(cmds: &[RenderCommand], s: &str) -> Color {
        let hits: Vec<Color> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, color, .. } if text == s => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(hits.len(), 1, "expected exactly one command reading {s:?}");
        hits[0]
    }

    #[test]
    fn every_colour_both_renderers_draw_comes_from_their_palette() {
        let mut drawn = 0;
        for light in [false, true] {
            let p = accented(light);
            for (state, name) in all_states() {
                let r = recorder_in(state);
                for (cmds, which) in [
                    (
                        render_recording_indicator(&r, &p, 4.0, 4.0),
                        "recording indicator",
                    ),
                    (
                        render_recording_controls(&r, &p, 0.0, 0.0, 400.0),
                        "recording controls",
                    ),
                ] {
                    drawn += cmds.len();
                    // `derived` is empty on purpose: this module computes no
                    // colour that is not either a role or a `readable_on`
                    // endpoint, and the endpoints are already allowed by the
                    // sweep. A module that needed an exception here would be
                    // claiming one, which is the point of the parameter.
                    assert_drawn_from(&p, &cmds, &[], &format!("{which} in {name}"));
                }
            }
        }
        // Non-vacuity. `assert_drawn_from` passes trivially on an empty
        // command list, and four of the six states legitimately produce one
        // from the indicator — so without this the whole sweep could be
        // green because nothing was ever drawn.
        assert!(drawn > 60, "the sweep only saw {drawn} commands");
    }

    #[test]
    fn none_of_the_ten_deleted_constants_is_still_drawn() {
        // Every constant this conversion deleted, by the name it had. The
        // light palette contains none of these values, so a leftover names
        // itself instead of merely failing.
        const DELETED: [(&str, u32); 10] = [
            ("MOCHA_BASE", 0x001E_1E2E),
            ("MOCHA_MANTLE", 0x0018_1825),
            ("MOCHA_SURFACE1", 0x0045_475A),
            ("MOCHA_TEXT", 0x00CD_D6F4),
            ("MOCHA_SUBTEXT0", 0x00A6_ADC8),
            ("MOCHA_GREEN", 0x00A6_E3A1),
            ("MOCHA_RED", 0x00F3_8BA8),
            ("MOCHA_YELLOW", 0x00F9_E2AF),
            ("MOCHA_PEACH", 0x00FA_B387),
            ("MOCHA_OVERLAY0", 0x006C_7086),
        ];
        let p = accented(true);
        for (state, name) in all_states() {
            let r = recorder_in(state);
            let mut cmds = render_recording_indicator(&r, &p, 4.0, 4.0);
            cmds.extend(render_recording_controls(&r, &p, 0.0, 0.0, 400.0));
            for c in colors(&cmds) {
                let rgb = (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
                for (cname, hex) in DELETED {
                    assert_ne!(
                        rgb, hex,
                        "the light render in {name} still draws {cname} (#{hex:06X}), \
                         so that constant survived the conversion"
                    );
                }
            }
        }
    }

    #[test]
    fn every_site_draws_the_role_it_claims() {
        for light in [false, true] {
            let p = accented(light);
            let mode = if light { "light" } else { "dark" };
            for (state, name) in all_states() {
                let r = recorder_in(state);

                // The whole command list, in draw order. Comparing the
                // sequence rather than the set is what catches a *permutation*
                // — two sites swapping roles leaves the set identical.
                let want_indicator: Vec<Color> = match state {
                    RecordingState::Recording => vec![p.panel_bg(), p.red, p.text, p.subtext0],
                    RecordingState::Paused => vec![p.panel_bg(), p.yellow, p.text, p.subtext0],
                    RecordingState::Processing => {
                        vec![p.panel_bg(), p.overlay0, p.text, p.subtext0]
                    }
                    _ => vec![],
                };
                assert_eq!(
                    colors(&render_recording_indicator(&r, &p, 4.0, 4.0)),
                    want_indicator,
                    "indicator in {name}, {mode} mode"
                );

                let stats = [p.subtext0, p.subtext0, p.subtext0];
                // `readable_on` here pins *which* rule the ink follows, and is
                // knowingly the same call the renderer makes. What the ink
                // actually comes out as, per mode, is pinned by hand in
                // `a_transport_button_is_lettered_for_its_own_fill` — without
                // that companion this line would be comparing the code to
                // itself.
                let want_controls: Vec<Color> = match state {
                    RecordingState::Idle => vec![p.mantle, p.text, p.red, readable_on(p.red)],
                    RecordingState::Recording => [
                        vec![
                            p.mantle,
                            p.text,
                            p.yellow,
                            readable_on(p.yellow),
                            p.surface1,
                            p.text,
                        ],
                        stats.to_vec(),
                    ]
                    .concat(),
                    RecordingState::Paused => [
                        vec![
                            p.mantle,
                            p.text,
                            p.green,
                            readable_on(p.green),
                            p.surface1,
                            p.text,
                        ],
                        stats.to_vec(),
                    ]
                    .concat(),
                    RecordingState::Processing => {
                        [vec![p.mantle, p.text, p.peach], stats.to_vec()].concat()
                    }
                    _ => vec![p.mantle, p.text, p.peach],
                };
                assert_eq!(
                    colors(&render_recording_controls(&r, &p, 0.0, 0.0, 400.0)),
                    want_controls,
                    "controls in {name}, {mode} mode"
                );
            }
        }
    }

    #[test]
    fn nothing_in_the_recorder_is_accented() {
        // Judgement 3. This cannot be checked by the pin table above: a table
        // names n sites and by construction cannot notice the n+1th, whereas a
        // count over the whole render can. It only means anything because
        // `accented()` puts the accent off both palettes.
        for light in [false, true] {
            let p = accented(light);
            for (state, name) in all_states() {
                let r = recorder_in(state);
                let mut cmds = render_recording_indicator(&r, &p, 4.0, 4.0);
                cmds.extend(render_recording_controls(&r, &p, 0.0, 0.0, 400.0));
                let n = colors(&cmds)
                    .into_iter()
                    .filter(|c| c.r == p.accent.r && c.g == p.accent.g && c.b == p.accent.b)
                    .count();
                assert_eq!(
                    n, 0,
                    "{n} accent-coloured commands in {name}; the transport \
                     colours are a code the user reads, not a theme"
                );
            }
        }
    }

    #[test]
    fn the_indicator_pill_is_as_transparent_as_the_user_asked() {
        // Judgement 1. The bug this replaces is invisible to every other test
        // here: `base` at a soldered alpha is still `base`, so the membership
        // sweep, the deleted-constant list and the pin table all pass it. Only
        // varying `panel_alpha` can see it — and `Palette::for_mode` always
        // sets 255, which is why the field is written directly.
        let r = recorder_in(RecordingState::Recording);
        let mut seen = Vec::new();
        for alpha in [255_u8, 200, 160] {
            let mut p = accented(false);
            p.panel_alpha = alpha;
            let cmds = render_recording_indicator(&r, &p, 0.0, 0.0);
            let cols = colors(&cmds);
            let pill = cols[0];
            assert_eq!(
                (pill.r, pill.g, pill.b),
                (p.base.r, p.base.g, p.base.b),
                "the pill stopped being `base`"
            );
            assert_eq!(pill.a, alpha, "the pill ignored panel_alpha = {alpha}");
            // And the dot does *not* thin out with it. A fix that made
            // everything follow the setting would be as wrong as the frozen
            // constant was: a recording indicator that fades until it cannot
            // be seen has failed at the one thing it is for.
            assert_eq!(cols[1].a, 255, "the recording dot faded with the panel");
            seen.push(pill.a);
        }
        assert_eq!(
            seen,
            vec![255, 200, 160],
            "the pill's alpha did not track the setting"
        );
    }

    #[test]
    fn a_transport_button_is_lettered_for_its_own_fill() {
        // Judgement 2, and the only conversion in this module that fixes a
        // bug a user could already see. The endpoints are written out rather
        // than obtained from `readable_on`, because a test that called the
        // same function the renderer calls would agree with it however wrong
        // both were.
        const NEAR_BLACK: u32 = 0x0011_111B;
        const NEAR_WHITE: u32 = 0x00EF_F1F5;
        let rgb = |c: Color| (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);

        for (state, label) in [
            (RecordingState::Idle, "Record"),
            (RecordingState::Recording, "Pause"),
            (RecordingState::Paused, "Resume"),
        ] {
            let r = recorder_in(state);
            let dark = accented(false);
            let light = accented(true);
            let ink_dark = text_color(
                &render_recording_controls(&r, &dark, 0.0, 0.0, 400.0),
                label,
            );
            let ink_light = text_color(
                &render_recording_controls(&r, &light, 0.0, 0.0, 400.0),
                label,
            );

            // Mocha's red, yellow and green are all pale enough that
            // near-black is the more legible endpoint on each, so all three
            // want dark ink.
            assert_eq!(
                rgb(ink_dark),
                NEAR_BLACK,
                "{label} in dark mode should be lettered near-black"
            );
            // The light theme's are all deep enough to want the opposite. This is exactly what the deleted
            // `MOCHA_BASE` ink could not do, and why it was a legibility bug
            // rather than merely an unconverted constant.
            assert_eq!(
                rgb(ink_light),
                NEAR_WHITE,
                "{label} in light mode should be lettered near-white"
            );
            assert_ne!(
                rgb(ink_dark),
                rgb(ink_light),
                "{label}'s ink did not move between the modes, so it is pinned \
                 rather than computed"
            );
        }
    }

    #[test]
    fn the_recording_dot_says_the_state_and_only_the_state() {
        // Judgement 3 at the one site where the colour *is* the information.
        for light in [false, true] {
            let p = accented(light);
            let dot = |state| {
                colors(&render_recording_indicator(
                    &recorder_in(state),
                    &p,
                    0.0,
                    0.0,
                ))[1]
            };
            let rec = dot(RecordingState::Recording);
            let pause = dot(RecordingState::Paused);
            let proc = dot(RecordingState::Processing);
            assert_eq!((rec.r, rec.g, rec.b), (p.red.r, p.red.g, p.red.b));
            assert_eq!(
                (pause.r, pause.g, pause.b),
                (p.yellow.r, p.yellow.g, p.yellow.b)
            );
            assert_eq!(
                (proc.r, proc.g, proc.b),
                (p.overlay0.r, p.overlay0.g, p.overlay0.b)
            );
            // Three states, three colours: a code with a collision in it is
            // not a code.
            assert_ne!((rec.r, rec.g, rec.b), (pause.r, pause.g, pause.b));
            assert_ne!((rec.r, rec.g, rec.b), (proc.r, proc.g, proc.b));
            assert_ne!((pause.r, pause.g, pause.b), (proc.r, proc.g, proc.b));

            // And the accent does not reach it. `nothing_is_accented` proves
            // the accent is absent; this proves the dot would not follow it
            // even if the accent changed, which is the claim about *meaning*
            // rather than about the current palette.
            let mut q = p;
            q.accent = Color::from_hex(0x00FF00);
            let moved = colors(&render_recording_indicator(
                &recorder_in(RecordingState::Recording),
                &q,
                0.0,
                0.0,
            ))[1];
            assert_eq!(
                (moved.r, moved.g, moved.b),
                (rec.r, rec.g, rec.b),
                "the recording dot followed the accent"
            );
        }
    }

    #[test]
    fn the_indicator_is_silent_unless_something_is_happening() {
        // Judgement 4 stated as a test, so that the four empty states above
        // are a checked fact rather than an assumption the colour tests were
        // quietly relying on.
        for light in [false, true] {
            let p = accented(light);
            for (state, name) in all_states() {
                let r = recorder_in(state);
                let drawn = !render_recording_indicator(&r, &p, 0.0, 0.0).is_empty();
                let expected = matches!(
                    state,
                    RecordingState::Recording | RecordingState::Paused | RecordingState::Processing
                );
                assert_eq!(drawn, expected, "indicator visibility in {name}");
                // The controls panel, by contrast, always draws: it is the
                // thing the user reaches for to leave the state they are in.
                assert!(
                    !render_recording_controls(&r, &p, 0.0, 0.0, 400.0).is_empty(),
                    "the controls panel vanished in {name}"
                );
            }
        }
    }

    // --- Format helpers ---
    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(1048576), "1.0 MiB");
        assert_eq!(format_bytes(1073741824), "1.0 GiB");
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
