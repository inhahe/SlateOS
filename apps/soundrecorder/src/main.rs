//! Slate OS Sound Recorder
//!
//! A recorder, and the place to look after what it records: the WAV files in
//! a recordings folder, any of which opens as a waveform of the whole file to
//! be marked and trimmed.
//!
//! **It cannot record or play here, and the window says so.** No application
//! can open a capture device -- the kernel's ALSA capture node hands back
//! silence, because the mixer behind it has no input -- and nothing gives an
//! application a way to play sound either. "No audio input" on its own reads as
//! an unplugged microphone and sends a user after a hardware fault they do not
//! have, so the window says it is this program that cannot, not the machine.
//! The take itself -- the state machine, the timer, the VU meter, the noise
//! gate, markers dropped as it runs, the save when it stops -- is written and
//! tested against samples the tests hand in, and waits on a capture source.
//!
//! **What works is everything after a take.** The recordings folder is listed
//! with each file's length and format, read through `wavpcm` from its first
//! megabyte. A recording opens as its whole waveform, drawn from the stored
//! samples; markers are added, named, moved and removed, and saved into the
//! file as the `cue ` and `labl` chunks sound editors share; a stretch is kept
//! by setting its start and end, and saved as a new file with its samples
//! copied as they are stored. The only file this ever writes over is the
//! recording whose markers are being saved -- and not if another program has
//! changed it since it was opened.

#![allow(dead_code, clippy::too_many_arguments, clippy::vec_init_then_push)]

use appearance::Palette;
use appearance::Surface;
use guitk::color::Color;
use guitk::dialog::{FileDialog, FilePicker, Picked};
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::textedit;
use guitk::textinput::TextInput;
use guitk::wheel;
use oswindow::app::{self, App, Response};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ============================================================================
// Recording state machine
// ============================================================================

/// The recording state machine: Idle -> Recording -> Paused -> Stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordingState {
    /// No recording in progress; ready to start.
    Idle,
    /// Actively capturing audio samples.
    Recording,
    /// Recording paused; can resume or stop.
    Paused,
    /// Recording finished; can play back or save.
    Stopped,
}

impl RecordingState {
    /// Returns the allowed transitions from the current state.
    pub fn allowed_transitions(self) -> &'static [RecordingState] {
        match self {
            Self::Idle => &[Self::Recording],
            Self::Recording => &[Self::Paused, Self::Stopped],
            Self::Paused => &[Self::Recording, Self::Stopped],
            Self::Stopped => &[Self::Idle],
        }
    }

    /// Whether a transition to the given target state is valid.
    pub fn can_transition_to(self, target: RecordingState) -> bool {
        self.allowed_transitions().contains(&target)
    }

    /// Human-readable label for this state.
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Recording => "Recording",
            Self::Paused => "Paused",
            Self::Stopped => "Stopped",
        }
    }

    /// Color associated with this state for UI display.
    pub fn color(self, pal: &Palette) -> Color {
        match self {
            Self::Idle => pal.subtext0,
            Self::Recording => pal.red,
            Self::Paused => pal.yellow,
            Self::Stopped => pal.green,
        }
    }
}

// ============================================================================
// Sample rate and quality presets
// ============================================================================

/// Supported sample rates in Hz.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleRate {
    Hz8000,
    Hz22050,
    Hz44100,
    Hz48000,
}

impl SampleRate {
    /// The numeric sample rate value.
    pub fn hz(self) -> u32 {
        match self {
            Self::Hz8000 => 8000,
            Self::Hz22050 => 22050,
            Self::Hz44100 => 44100,
            Self::Hz48000 => 48000,
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Hz8000 => "8,000 Hz",
            Self::Hz22050 => "22,050 Hz",
            Self::Hz44100 => "44,100 Hz",
            Self::Hz48000 => "48,000 Hz",
        }
    }

    /// All available sample rates.
    pub fn all() -> &'static [SampleRate] {
        &[Self::Hz8000, Self::Hz22050, Self::Hz44100, Self::Hz48000]
    }

    /// Bytes per second for mono 16-bit PCM at this rate.
    pub fn bytes_per_second_mono(self) -> u32 {
        self.hz().saturating_mul(2) // 16 bits = 2 bytes per sample
    }

    /// Bytes per second for stereo 16-bit PCM at this rate.
    pub fn bytes_per_second_stereo(self) -> u32 {
        self.hz().saturating_mul(4) // 2 channels * 2 bytes per sample
    }
}

/// Recording quality presets that bundle sample rate and channel config.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityPreset {
    /// Low-bandwidth voice recording: 8 kHz mono.
    Voice,
    /// Standard music quality: 44.1 kHz stereo.
    Music,
    /// Maximum fidelity: 48 kHz stereo.
    Lossless,
}

impl QualityPreset {
    pub fn sample_rate(self) -> SampleRate {
        match self {
            Self::Voice => SampleRate::Hz8000,
            Self::Music => SampleRate::Hz44100,
            Self::Lossless => SampleRate::Hz48000,
        }
    }

    pub fn channels(self) -> u16 {
        match self {
            Self::Voice => 1,
            Self::Music | Self::Lossless => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Voice => "Voice (8 kHz Mono)",
            Self::Music => "Music (44.1 kHz Stereo)",
            Self::Lossless => "Lossless (48 kHz Stereo)",
        }
    }

    /// All available presets.
    pub fn all() -> &'static [QualityPreset] {
        &[Self::Voice, Self::Music, Self::Lossless]
    }

    /// Bits per sample (always 16 for PCM).
    pub fn bits_per_sample(self) -> u16 {
        16
    }

    /// Bytes per second at this preset's settings.
    pub fn bytes_per_second(self) -> u32 {
        let sample_bytes: u32 = 2; // 16 bits
        self.sample_rate()
            .hz()
            .saturating_mul(u32::from(self.channels()))
            .saturating_mul(sample_bytes)
    }
}

// ============================================================================
// Audio input device
// ============================================================================

/// Why no take can start, in one line: drawn beside the transport when
/// Record is pressed.
const CANNOT_RECORD: &str = "Cannot record: no application can open a capture device here yet";

/// Why a recording cannot be heard, drawn when Play is pressed.
const CANNOT_PLAY: &str = "Cannot play: no application can send sound to a device here yet";

/// What the window says above everything else, before anything is pressed.
///
/// Two lines. The second is the one that matters: "no audio input" on its own
/// reads as an unplugged microphone, and sends the user looking for a fault in
/// hardware they do not have.
const NO_AUDIO_LINES: [&str; 2] = [
    "This recorder cannot record or play sound here.",
    "Not a missing microphone: no application can open an audio device yet. Recordings on disk can be opened, marked and trimmed.",
];

/// Represents an audio input source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioInputDevice {
    /// Unique device identifier.
    pub id: u32,
    /// Human-readable name.
    pub name: String,
    /// Whether this device is the system default.
    pub is_default: bool,
    /// Number of input channels available.
    pub max_channels: u16,
}

impl AudioInputDevice {
    /// Create a list of mock input devices for the UI.
    /// Three plausible microphones, for tests.
    ///
    /// `#[cfg(test)]` since 2026-09-15. `App::new` called this, so the device
    /// menu listed a built-in microphone, a USB interface and a line input,
    /// none of which had been enumerated from anything -- this crate has no
    /// audio device access, no `std::fs` and no syscall that could find one.
    #[cfg(test)]
    pub fn mock_devices() -> Vec<AudioInputDevice> {
        vec![
            AudioInputDevice {
                id: 0,
                name: "Built-in Microphone".into(),
                is_default: true,
                max_channels: 2,
            },
            AudioInputDevice {
                id: 1,
                name: "USB Audio Interface".into(),
                is_default: false,
                max_channels: 2,
            },
            AudioInputDevice {
                id: 2,
                name: "Webcam Microphone".into(),
                is_default: false,
                max_channels: 1,
            },
            AudioInputDevice {
                id: 3,
                name: "Line In".into(),
                is_default: false,
                max_channels: 2,
            },
        ]
    }
}

// ============================================================================
// WAV file format
// ============================================================================

/// WAV file header and data generator for PCM 16-bit audio.
///
/// WAV format: RIFF header, "fmt " sub-chunk, "data" sub-chunk.
/// We generate standard RIFF WAVE files with PCM encoding (format tag 1).
pub struct WavFile {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub samples: Vec<i16>,
}

impl WavFile {
    /// Create a new empty WAV file with the given audio parameters.
    pub fn new(sample_rate: u32, channels: u16, bits_per_sample: u16) -> Self {
        Self {
            sample_rate,
            channels,
            bits_per_sample,
            samples: Vec::new(),
        }
    }

    /// Create from a quality preset.
    pub fn from_preset(preset: QualityPreset) -> Self {
        Self::new(
            preset.sample_rate().hz(),
            preset.channels(),
            preset.bits_per_sample(),
        )
    }

    /// Append samples to the recording.
    pub fn push_samples(&mut self, data: &[i16]) {
        self.samples.extend_from_slice(data);
    }

    /// Total number of sample frames (samples / channels).
    pub fn frame_count(&self) -> usize {
        let ch = self.channels as usize;
        if ch == 0 {
            return 0;
        }
        self.samples.len().checked_div(ch).unwrap_or(0)
    }

    /// Duration in seconds.
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.frame_count() as f64 / self.sample_rate as f64
    }

    /// Block align: channels * (bits_per_sample / 8).
    pub fn block_align(&self) -> u16 {
        self.channels.saturating_mul(self.bits_per_sample / 8)
    }

    /// Byte rate: sample_rate * block_align.
    pub fn byte_rate(&self) -> u32 {
        self.sample_rate
            .saturating_mul(u32::from(self.block_align()))
    }

    /// Size of the raw PCM data in bytes.
    pub fn data_size(&self) -> u32 {
        u32::try_from(self.samples.len().saturating_mul(2)).unwrap_or(u32::MAX)
    }

    /// The take as a 16-bit WAV file: its samples stored exactly as they were
    /// captured -- `wavpcm::encode` would dither them -- and the markers
    /// dropped as it ran as the file's own `cue ` chunk, which any sound
    /// editor reads.
    ///
    /// It wrote its own header before, and a reader of its own that took only
    /// a 44-byte canonical header; `wavpcm` reads and writes every WAV here.
    ///
    /// # Errors
    ///
    /// When the take is longer than a WAV file can hold.
    pub fn to_wav(&self, markers: &MarkerList) -> Result<Vec<u8>, wavpcm::WavError> {
        let bytes = wavpcm::encode_pcm16(self.sample_rate, self.channels, &self.samples)?;
        let cues: Vec<wavpcm::Cue> = markers
            .sorted()
            .iter()
            .map(|m| wavpcm::Cue {
                frame: u32::try_from(m.frame_position).unwrap_or(u32::MAX),
                label: m.label.clone().into_bytes(),
            })
            .collect();
        wavpcm::with_cues(&bytes, &cues)
    }
}

// ============================================================================
// Waveform visualization
// ============================================================================

/// Scrolling waveform display that shows recent audio amplitude.
pub struct WaveformDisplay {
    /// Circular buffer of amplitude samples (0.0..1.0) for display.
    amplitudes: VecDeque<f32>,
    /// Maximum number of amplitude columns to display.
    max_columns: usize,
    /// Display area dimensions.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl WaveformDisplay {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        let max_columns = (width as usize).max(1);
        Self {
            amplitudes: VecDeque::with_capacity(max_columns),
            max_columns,
            x,
            y,
            width,
            height,
        }
    }

    /// Push a new amplitude value (0.0..1.0) to the display.
    pub fn push_amplitude(&mut self, amplitude: f32) {
        let clamped = amplitude.clamp(0.0, 1.0);
        if self.amplitudes.len() >= self.max_columns {
            self.amplitudes.pop_front();
        }
        self.amplitudes.push_back(clamped);
    }

    /// Reduce a block of raw samples to a single amplitude for display.
    pub fn amplitude_from_samples(samples: &[i16]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let peak = samples
            .iter()
            .map(|&s| (s as f32).abs())
            .fold(0.0f32, f32::max);
        (peak / 32768.0).clamp(0.0, 1.0)
    }

    /// Clear the waveform display.
    pub fn clear(&mut self) {
        self.amplitudes.clear();
    }

    /// Number of amplitude columns currently stored.
    pub fn len(&self) -> usize {
        self.amplitudes.len()
    }

    /// Whether the display is empty.
    pub fn is_empty(&self) -> bool {
        self.amplitudes.is_empty()
    }

    /// Render the waveform visualization to render commands.
    pub fn render(&self, pal: &Palette) -> Vec<RenderCommand> {
        let mut commands = Vec::new();

        // Background
        pal.push_surface(
            &mut commands,
            self.x,
            self.y,
            self.width,
            self.height,
            4.0,
            Surface::Card,
        );

        // Center line
        let center_y = self.y + self.height / 2.0;
        commands.push(RenderCommand::Line {
            x1: self.x,
            y1: center_y,
            x2: self.x + self.width,
            y2: center_y,
            color: pal.overlay0,
            width: 1.0,
        });

        // Waveform bars
        let col_count = self.amplitudes.len();
        if col_count > 0 {
            let bar_width = self.width / self.max_columns as f32;
            let max_bar_height = self.height / 2.0 - 2.0;
            let start_offset = self.max_columns.saturating_sub(col_count) as f32 * bar_width;

            for (i, &amp) in self.amplitudes.iter().enumerate() {
                let bar_h = amp * max_bar_height;
                if bar_h < 0.5 {
                    continue;
                }
                let bx = self.x + start_offset + i as f32 * bar_width;

                // Draw symmetric bars above and below center
                commands.push(RenderCommand::FillRect {
                    x: bx,
                    y: center_y - bar_h,
                    width: bar_width.max(1.0),
                    height: bar_h * 2.0,
                    color: pal.green,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }

        // Border
        commands.push(RenderCommand::StrokeRect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            color: pal.surface0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });

        commands
    }
}

// ============================================================================
// VU meter
// ============================================================================

/// Peak level VU meter with exponential decay.
pub struct VuMeter {
    /// Current displayed level (0.0..1.0), decays over time.
    pub current_level: f32,
    /// Current peak hold level (0.0..1.0).
    pub peak_level: f32,
    /// Ticks remaining for peak hold before it starts decaying.
    peak_hold_ticks: u32,
    /// Decay rate per tick for the current level (multiplier < 1.0).
    decay_rate: f32,
    /// Peak hold duration in ticks before decay begins.
    peak_hold_duration: u32,
    /// Display dimensions.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl VuMeter {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            current_level: 0.0,
            peak_level: 0.0,
            peak_hold_ticks: 0,
            decay_rate: 0.95,
            peak_hold_duration: 30,
            x,
            y,
            width,
            height,
        }
    }

    /// Feed a new peak amplitude value (0.0..1.0).
    pub fn update(&mut self, amplitude: f32) {
        let amp = amplitude.clamp(0.0, 1.0);

        // Instant attack, exponential decay
        if amp > self.current_level {
            self.current_level = amp;
        } else {
            self.current_level *= self.decay_rate;
        }

        // Peak hold
        if amp > self.peak_level {
            self.peak_level = amp;
            self.peak_hold_ticks = self.peak_hold_duration;
        } else if self.peak_hold_ticks > 0 {
            self.peak_hold_ticks = self.peak_hold_ticks.saturating_sub(1);
        } else {
            self.peak_level *= self.decay_rate;
        }
    }

    /// Reset the meter to zero.
    pub fn reset(&mut self) {
        self.current_level = 0.0;
        self.peak_level = 0.0;
        self.peak_hold_ticks = 0;
    }

    /// Convert a level to a display color (green -> yellow -> red).
    fn level_color(level: f32, pal: &Palette) -> Color {
        if level < 0.6 {
            pal.green
        } else if level < 0.85 {
            pal.yellow
        } else {
            pal.red
        }
    }

    /// Render the VU meter.
    pub fn render(&self, pal: &Palette) -> Vec<RenderCommand> {
        let mut commands = Vec::new();

        // Background
        pal.push_surface(
            &mut commands,
            self.x,
            self.y,
            self.width,
            self.height,
            3.0,
            Surface::ControlTrack,
        );

        // Level bar
        let bar_width = self.current_level * (self.width - 4.0);
        if bar_width > 0.5 {
            commands.push(RenderCommand::FillRect {
                x: self.x + 2.0,
                y: self.y + 2.0,
                width: bar_width,
                height: self.height - 4.0,
                color: Self::level_color(self.current_level, pal),
                corner_radii: CornerRadii::all(2.0),
            });
        }

        // Peak indicator line
        if self.peak_level > 0.01 {
            let peak_x = self.x + 2.0 + self.peak_level * (self.width - 4.0);
            commands.push(RenderCommand::Line {
                x1: peak_x,
                y1: self.y + 1.0,
                x2: peak_x,
                y2: self.y + self.height - 1.0,
                color: pal.red,
                width: 2.0,
            });
        }

        // Border
        commands.push(RenderCommand::StrokeRect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            color: pal.surface0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(3.0),
        });

        commands
    }
}

// ============================================================================
// Recording timer
// ============================================================================

/// Tracks elapsed recording time and estimates remaining space.
pub struct RecordingTimer {
    /// Elapsed recording time in milliseconds.
    elapsed_ms: u64,
    /// Available disk space in bytes, if anything has measured it.
    ///
    /// `Option` since 2026-09-15. It was a `u64` initialised to 1_000_000_000
    /// -- a flat gigabyte nothing had looked up -- and the transport displayed
    /// `-01:26:48` beside the elapsed clock as the time left to record. That
    /// is a number someone plans a session around: it says whether the take
    /// will fit. A `u64` has no way to say "unmeasured", so the only available
    /// answer was a wrong one.
    available_bytes: Option<u64>,
    /// Current bytes-per-second rate for space estimation.
    bytes_per_second: u32,
}

impl RecordingTimer {
    pub fn new(available_bytes: Option<u64>, bytes_per_second: u32) -> Self {
        Self {
            elapsed_ms: 0,
            available_bytes,
            bytes_per_second,
        }
    }

    /// Advance the timer by the given number of milliseconds.
    pub fn tick(&mut self, delta_ms: u64) {
        self.elapsed_ms = self.elapsed_ms.saturating_add(delta_ms);
    }

    /// Reset the timer to zero.
    pub fn reset(&mut self) {
        self.elapsed_ms = 0;
    }

    /// Get elapsed time in seconds.
    pub fn elapsed_secs(&self) -> f64 {
        self.elapsed_ms as f64 / 1000.0
    }

    /// Format elapsed time as HH:MM:SS.
    pub fn format_elapsed(&self) -> String {
        let total_secs = (self.elapsed_ms / 1000) as u32;
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    }

    /// Estimate remaining recording time in seconds based on available space.
    ///
    /// `None` when nothing has measured the free space, which is the only
    /// state this program can currently be in.
    pub fn remaining_secs(&self) -> Option<f64> {
        if self.bytes_per_second == 0 {
            return Some(f64::INFINITY);
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a byte count displayed to the second"
        )]
        Some(self.available_bytes? as f64 / f64::from(self.bytes_per_second))
    }

    /// Format remaining time as a human-readable string.
    ///
    /// `--:--:--` when the free space is unmeasured. Deliberately not `00:00:00`,
    /// which says the disk is full and the take is about to stop, and not an
    /// omitted field, which is read as however much room you like.
    pub fn format_remaining(&self) -> String {
        let Some(secs) = self.remaining_secs() else {
            return "--:--:--".into();
        };
        if secs.is_infinite() || secs > 359_999.0 {
            return "99:59:59+".into();
        }
        let total_secs = secs as u32;
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    }

    /// Update the available space (e.g., after writing a chunk).
    pub fn set_available_bytes(&mut self, bytes: u64) {
        self.available_bytes = Some(bytes);
    }

    /// Update the byte rate (e.g., after changing quality preset).
    pub fn set_bytes_per_second(&mut self, bps: u32) {
        self.bytes_per_second = bps;
    }

    /// Render the timer display.
    pub fn render(&self, pal: &Palette, x: f32, y: f32) -> Vec<RenderCommand> {
        let elapsed_text = self.format_elapsed();
        // No leading minus when the figure is unknown: "-​--:--:--" reads as a
        // negative duration rather than an absent one.
        let remaining_text = if self.remaining_secs().is_some() {
            format!("-{}", self.format_remaining())
        } else {
            self.format_remaining()
        };

        vec![
            // Elapsed time (large)
            RenderCommand::Text {
                x,
                y,
                text: elapsed_text,
                color: pal.text,
                font_size: 28.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            },
            // Remaining label
            RenderCommand::Text {
                x: x + 200.0,
                y: y + 6.0,
                text: remaining_text,
                color: pal.subtext0,
                font_size: 16.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            },
        ]
    }
}

// ============================================================================
// Markers / bookmarks
// ============================================================================

/// A bookmark placed at a specific point during recording.
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    /// Marker identifier.
    pub id: u32,
    /// Position in sample frames from the start.
    pub frame_position: u64,
    /// Optional user-provided label.
    pub label: String,
    /// Color for visual display.
    pub color: Color,
}

/// Manages a list of markers associated with a recording.
pub struct MarkerList {
    markers: Vec<Marker>,
    next_id: u32,
}

impl MarkerList {
    pub fn new() -> Self {
        Self {
            markers: Vec::new(),
            next_id: 0,
        }
    }

    /// Add a marker at the given frame position.
    pub fn add(&mut self, frame_position: u64, label: String) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let color_index = id as usize % 4;
        // Fixed values, not theme roles. A marker stores its colour, so it is
        // the user's own mark: nothing rewrites it when the theme changes, and
        // a themed value would leave markers dropped before the change in the
        // old scheme and ones dropped after in the new. Same rule as
        // `kanban`'s labels and `hexeditor`'s bookmarks.
        let color = match color_index {
            0 => Color::from_hex(0x89B4FA),
            1 => Color::from_hex(0xFAB387),
            2 => Color::from_hex(0xF9E2AF),
            _ => Color::from_hex(0xA6E3A1),
        };
        self.markers.push(Marker {
            id,
            frame_position,
            label,
            color,
        });
        id
    }

    /// Remove a marker by id. Returns true if found and removed.
    pub fn remove(&mut self, id: u32) -> bool {
        let len_before = self.markers.len();
        self.markers.retain(|m| m.id != id);
        self.markers.len() < len_before
    }

    /// Get all markers, sorted by frame position.
    pub fn sorted(&self) -> Vec<&Marker> {
        let mut refs: Vec<_> = self.markers.iter().collect();
        refs.sort_by_key(|m| m.frame_position);
        refs
    }

    /// Number of markers.
    pub fn len(&self) -> usize {
        self.markers.len()
    }

    /// Whether the marker list is empty.
    pub fn is_empty(&self) -> bool {
        self.markers.is_empty()
    }

    /// Clear all markers.
    pub fn clear(&mut self) {
        self.markers.clear();
    }

    /// Find a marker by id.
    pub fn get(&self, id: u32) -> Option<&Marker> {
        self.markers.iter().find(|m| m.id == id)
    }

    /// Render markers as vertical lines over a waveform region.
    pub fn render(
        &self,
        total_frames: u64,
        region_x: f32,
        region_y: f32,
        region_width: f32,
        region_height: f32,
    ) -> Vec<RenderCommand> {
        let mut commands = Vec::new();
        if total_frames == 0 {
            return commands;
        }

        for marker in &self.markers {
            let frac = marker.frame_position as f32 / total_frames as f32;
            let mx = region_x + frac * region_width;

            // Vertical marker line
            commands.push(RenderCommand::Line {
                x1: mx,
                y1: region_y,
                x2: mx,
                y2: region_y + region_height,
                color: marker.color,
                width: 2.0,
            });

            // Label above
            if !marker.label.is_empty() {
                commands.push(RenderCommand::Text {
                    x: mx + 3.0,
                    y: region_y + 2.0,
                    text: marker.label.clone(),
                    color: marker.color,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(80.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }

        commands
    }
}

impl Default for MarkerList {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Trim tool
// ============================================================================

/// Defines a trim region with start and end points to crop a recording.
#[derive(Clone, Debug, PartialEq)]
pub struct TrimRegion {
    /// Start frame (inclusive).
    pub start_frame: u64,
    /// End frame (exclusive).
    pub end_frame: u64,
    /// Total frames in the recording.
    pub total_frames: u64,
}

impl TrimRegion {
    /// Create a new trim region spanning the full recording.
    pub fn full(total_frames: u64) -> Self {
        Self {
            start_frame: 0,
            end_frame: total_frames,
            total_frames,
        }
    }

    /// Set the start point (clamped to valid range).
    pub fn set_start(&mut self, frame: u64) {
        self.start_frame = frame.min(self.end_frame.saturating_sub(1));
    }

    /// Set the end point (clamped to valid range).
    pub fn set_end(&mut self, frame: u64) {
        let clamped = frame.min(self.total_frames);
        self.end_frame = clamped.max(self.start_frame.saturating_add(1));
    }

    /// Number of frames in the trimmed region.
    pub fn length_frames(&self) -> u64 {
        self.end_frame.saturating_sub(self.start_frame)
    }

    /// Duration of the trimmed region in seconds.
    pub fn duration_secs(&self, sample_rate: u32) -> f64 {
        if sample_rate == 0 {
            return 0.0;
        }
        self.length_frames() as f64 / sample_rate as f64
    }

    /// Whether the full recording is selected (no trimming).
    pub fn is_full(&self) -> bool {
        self.start_frame == 0 && self.end_frame == self.total_frames
    }

    /// Apply the trim to a sample buffer (assumes interleaved channels).
    pub fn apply(&self, samples: &[i16], channels: u16) -> Vec<i16> {
        let ch = channels as usize;
        if ch == 0 {
            return Vec::new();
        }
        let start_idx = (self.start_frame as usize).saturating_mul(ch);
        let end_idx = (self.end_frame as usize)
            .saturating_mul(ch)
            .min(samples.len());
        if start_idx >= samples.len() || start_idx >= end_idx {
            return Vec::new();
        }
        samples
            .get(start_idx..end_idx)
            .map(<[i16]>::to_vec)
            .unwrap_or_default()
    }

    /// Render the trim handles on a waveform region.
    pub fn render(
        &self,
        pal: &Palette,
        region_x: f32,
        region_y: f32,
        region_width: f32,
        region_height: f32,
    ) -> Vec<RenderCommand> {
        let mut commands = Vec::new();
        if self.total_frames == 0 {
            return commands;
        }

        let start_frac = self.start_frame as f32 / self.total_frames as f32;
        let end_frac = self.end_frame as f32 / self.total_frames as f32;
        let start_x = region_x + start_frac * region_width;
        let end_x = region_x + end_frac * region_width;

        // Dimmed region before start
        if start_frac > 0.0 {
            commands.push(RenderCommand::FillRect {
                x: region_x,
                y: region_y,
                width: start_x - region_x,
                height: region_height,
                color: Color::rgba(0, 0, 0, 128),
                corner_radii: CornerRadii::ZERO,
            });
        }

        // Dimmed region after end
        if end_frac < 1.0 {
            commands.push(RenderCommand::FillRect {
                x: end_x,
                y: region_y,
                width: region_x + region_width - end_x,
                height: region_height,
                color: Color::rgba(0, 0, 0, 128),
                corner_radii: CornerRadii::ZERO,
            });
        }

        // Start handle
        commands.push(RenderCommand::FillRect {
            x: start_x - 3.0,
            y: region_y,
            width: 6.0,
            height: region_height,
            color: pal.blue,
            corner_radii: CornerRadii::ZERO,
        });

        // End handle
        commands.push(RenderCommand::FillRect {
            x: end_x - 3.0,
            y: region_y,
            width: 6.0,
            height: region_height,
            color: pal.blue,
            corner_radii: CornerRadii::ZERO,
        });

        commands
    }
}

// ============================================================================
// Noise gate
// ============================================================================

/// Simple noise gate that suppresses audio below a threshold.
pub struct NoiseGate {
    /// Threshold level (0.0..1.0). Samples below this are zeroed.
    pub threshold: f32,
    /// Whether the gate is currently enabled.
    pub enabled: bool,
    /// Whether the gate is currently open (signal is above threshold).
    pub is_open: bool,
    /// Attack time in samples (how quickly the gate opens).
    attack_samples: u32,
    /// Release time in samples (how long to keep open after signal drops).
    release_samples: u32,
    /// Counter for release timing.
    release_counter: u32,
}

impl NoiseGate {
    pub fn new(threshold: f32) -> Self {
        Self {
            threshold: threshold.clamp(0.0, 1.0),
            enabled: true,
            is_open: false,
            attack_samples: 64,
            release_samples: 4800, // ~100ms at 48kHz
            release_counter: 0,
        }
    }

    /// Set the threshold (0.0..1.0).
    pub fn set_threshold(&mut self, threshold: f32) {
        self.threshold = threshold.clamp(0.0, 1.0);
    }

    /// Process a block of samples in place through the noise gate.
    /// Returns true if any audio passed through (gate was open).
    pub fn process(&mut self, samples: &mut [i16]) -> bool {
        if !self.enabled {
            return true;
        }

        let mut any_passed = false;
        let threshold_i16 = (self.threshold * 32767.0) as i16;

        for sample in samples.iter_mut() {
            let abs_sample = sample.saturating_abs();

            if abs_sample > threshold_i16 {
                // Signal above threshold: open the gate
                self.is_open = true;
                self.release_counter = self.release_samples;
                any_passed = true;
            } else if self.release_counter > 0 {
                // In release period: keep gate open
                self.release_counter = self.release_counter.saturating_sub(1);
                any_passed = true;
            } else {
                // Gate closed: zero the sample
                self.is_open = false;
                *sample = 0;
            }
        }

        any_passed
    }

    /// Reset gate state.
    pub fn reset(&mut self) {
        self.is_open = false;
        self.release_counter = 0;
    }

    /// Render the noise gate threshold indicator.
    pub fn render(&self, pal: &Palette, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut commands = Vec::new();

        // Label
        commands.push(RenderCommand::Text {
            x,
            y,
            text: format!("Noise Gate: {:.0}%", self.threshold * 100.0),
            color: if self.enabled { pal.text } else { pal.overlay0 },
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Threshold slider track
        let track_y = y + 18.0;
        pal.push_surface(
            &mut commands,
            x,
            track_y,
            width,
            6.0,
            3.0,
            Surface::ControlTrack,
        );

        // Threshold position
        let knob_x = x + self.threshold * width;
        commands.push(RenderCommand::FillRect {
            x: knob_x - 4.0,
            y: track_y - 4.0,
            width: 8.0,
            height: 14.0,
            color: if self.enabled {
                pal.peach
            } else {
                pal.overlay0
            },
            corner_radii: CornerRadii::all(4.0),
        });

        // Gate status indicator
        let status_color = if !self.enabled {
            pal.overlay0
        } else if self.is_open {
            pal.green
        } else {
            pal.red
        };
        commands.push(RenderCommand::FillRect {
            x: x + width + 10.0,
            y: track_y - 1.0,
            width: 8.0,
            height: 8.0,
            color: status_color,
            corner_radii: CornerRadii::all(4.0),
        });

        commands
    }
}

// ============================================================================
// Playback state
// ============================================================================

/// Playback state for reviewing recorded audio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

/// Playback controller for reviewing recordings.
pub struct PlaybackController {
    /// Current playback state.
    pub state: PlaybackState,
    /// Current playback position in sample frames.
    pub position_frames: u64,
    /// Total frames in the loaded recording.
    pub total_frames: u64,
    /// Sample rate of the loaded recording.
    pub sample_rate: u32,
}

impl PlaybackController {
    pub fn new() -> Self {
        Self {
            state: PlaybackState::Stopped,
            position_frames: 0,
            total_frames: 0,
            sample_rate: 48000,
        }
    }

    /// Load a recording for playback.
    pub fn load(&mut self, total_frames: u64, sample_rate: u32) {
        self.total_frames = total_frames;
        self.sample_rate = sample_rate;
        self.position_frames = 0;
        self.state = PlaybackState::Stopped;
    }

    /// Start or resume playback.
    pub fn play(&mut self) {
        if self.total_frames > 0 {
            self.state = PlaybackState::Playing;
        }
    }

    /// Pause playback.
    pub fn pause(&mut self) {
        if self.state == PlaybackState::Playing {
            self.state = PlaybackState::Paused;
        }
    }

    /// Stop playback and reset position.
    pub fn stop(&mut self) {
        self.state = PlaybackState::Stopped;
        self.position_frames = 0;
    }

    /// Seek to a specific frame position.
    pub fn seek(&mut self, frame: u64) {
        self.position_frames = frame.min(self.total_frames);
    }

    /// Seek by a time offset in seconds (positive = forward, negative = backward).
    pub fn seek_relative(&mut self, delta_secs: f64) {
        let delta_frames = (delta_secs * self.sample_rate as f64) as i64;
        let new_pos = (self.position_frames as i64)
            .saturating_add(delta_frames)
            .max(0) as u64;
        self.position_frames = new_pos.min(self.total_frames);
    }

    /// Advance playback position by the given number of frames.
    /// Returns true if playback reached the end.
    pub fn advance(&mut self, frames: u64) -> bool {
        if self.state != PlaybackState::Playing {
            return false;
        }
        self.position_frames = self.position_frames.saturating_add(frames);
        if self.position_frames >= self.total_frames {
            self.position_frames = self.total_frames;
            self.state = PlaybackState::Stopped;
            return true;
        }
        false
    }

    /// Current position as a fraction (0.0..1.0).
    pub fn progress(&self) -> f32 {
        if self.total_frames == 0 {
            return 0.0;
        }
        self.position_frames as f32 / self.total_frames as f32
    }

    /// Current position formatted as MM:SS.
    pub fn format_position(&self) -> String {
        if self.sample_rate == 0 {
            return "00:00".into();
        }
        let secs = self
            .position_frames
            .checked_div(u64::from(self.sample_rate))
            .unwrap_or(0) as u32;
        let minutes = secs / 60;
        let seconds = secs % 60;
        format!("{minutes:02}:{seconds:02}")
    }

    /// Total duration formatted as MM:SS.
    pub fn format_duration(&self) -> String {
        if self.sample_rate == 0 {
            return "00:00".into();
        }
        let secs = self
            .total_frames
            .checked_div(u64::from(self.sample_rate))
            .unwrap_or(0) as u32;
        let minutes = secs / 60;
        let seconds = secs % 60;
        format!("{minutes:02}:{seconds:02}")
    }

    /// Render the playback bar.
    pub fn render(&self, pal: &Palette, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut commands = Vec::new();
        let bar_height = 6.0;
        let bar_y = y + 10.0;

        // Track background
        pal.push_surface(
            &mut commands,
            x,
            bar_y,
            width,
            bar_height,
            3.0,
            Surface::ControlTrack,
        );

        // Progress fill
        let fill_width = self.progress() * width;
        if fill_width > 0.5 {
            commands.push(RenderCommand::FillRect {
                x,
                y: bar_y,
                width: fill_width,
                height: bar_height,
                color: pal.blue,
                corner_radii: CornerRadii::all(3.0),
            });
        }

        // Position indicator
        let knob_x = x + fill_width;
        commands.push(RenderCommand::FillRect {
            x: knob_x - 5.0,
            y: bar_y - 4.0,
            width: 10.0,
            height: bar_height + 8.0,
            color: pal.text,
            corner_radii: CornerRadii::all(5.0),
        });

        // Time labels
        commands.push(RenderCommand::Text {
            x,
            y: bar_y + bar_height + 6.0,
            text: self.format_position(),
            color: pal.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        commands.push(RenderCommand::Text {
            x: x + width - 40.0,
            y: bar_y + bar_height + 6.0,
            text: self.format_duration(),
            color: pal.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        commands
    }
}

impl Default for PlaybackController {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Auto-save
// ============================================================================

/// Manages periodic auto-saving of long recordings.
pub struct AutoSave {
    /// Auto-save interval in milliseconds (0 = disabled).
    pub interval_ms: u64,
    /// Milliseconds since the last auto-save.
    elapsed_since_save_ms: u64,
    /// Whether auto-save is enabled.
    pub enabled: bool,
    /// Number of auto-saves performed in this session.
    pub save_count: u32,
}

impl AutoSave {
    /// Create with the given interval in seconds.
    pub fn new(interval_secs: u32) -> Self {
        Self {
            interval_ms: (interval_secs as u64).saturating_mul(1000),
            elapsed_since_save_ms: 0,
            enabled: interval_secs > 0,
            save_count: 0,
        }
    }

    /// Tick the auto-save timer. Returns true if it is time to save.
    pub fn tick(&mut self, delta_ms: u64) -> bool {
        if !self.enabled || self.interval_ms == 0 {
            return false;
        }
        self.elapsed_since_save_ms = self.elapsed_since_save_ms.saturating_add(delta_ms);
        if self.elapsed_since_save_ms >= self.interval_ms {
            self.elapsed_since_save_ms =
                self.elapsed_since_save_ms.saturating_sub(self.interval_ms);
            self.save_count = self.save_count.saturating_add(1);
            return true;
        }
        false
    }

    /// Reset the timer (e.g., after a manual save).
    pub fn reset_timer(&mut self) {
        self.elapsed_since_save_ms = 0;
    }

    /// Set the interval in seconds.
    pub fn set_interval_secs(&mut self, secs: u32) {
        self.interval_ms = (secs as u64).saturating_mul(1000);
        self.enabled = secs > 0;
    }
}

// ============================================================================
// The recordings folder
// ============================================================================

/// How much of a file is read to list it: its header, and the start of its
/// samples.
const HEADER_BYTES: usize = 1 << 20;

/// The largest recording this opens -- an hour and forty minutes of
/// CD-quality stereo. It is held whole, to draw it and to cut from it.
const MAX_RECORDING_BYTES: usize = 1 << 30;

/// How many stretches a recording's waveform is measured in when it opens;
/// the window draws them at whatever width it has.
const PEAK_COLUMNS: usize = 4096;

/// A recording in the folder.
#[derive(Clone, Debug, PartialEq)]
pub struct LibraryEntry {
    pub path: PathBuf,
    /// The file's name, as it is shown.
    pub name: String,
    pub size: u64,
    /// Its length and format -- or why it is not a WAV this reads.
    pub detail: Result<wavpcm::Info, String>,
}

impl LibraryEntry {
    /// What `path` is, read from its first part.
    ///
    /// # Errors
    ///
    /// When it cannot be read at all.
    pub fn read(path: &Path) -> Result<Self, String> {
        let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let head = safeio::read_capped(path, HEADER_BYTES)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self {
            path: path.to_path_buf(),
            name: shown_name(path),
            size: meta.len(),
            detail: wavpcm::parse_header_prefix(&head.bytes, meta.len()).map_err(|e| e.to_string()),
        })
    }

    /// The line under its name: its length and format, or why it will not
    /// open.
    pub fn summary(&self) -> String {
        match &self.detail {
            Ok(info) => format!(
                "{}  \u{00B7}  {}  \u{00B7}  {}",
                clock(info.seconds()),
                format_line(info),
                guitk::bytes::iec(self.size)
            ),
            Err(why) => format!("Will not open: {why}"),
        }
    }
}

/// The WAV files in `dir`, in the order of their names.
///
/// # Errors
///
/// When the folder cannot be listed.
pub fn scan(dir: &Path) -> Result<Vec<LibraryEntry>, String> {
    let listing = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out = Vec::new();
    for entry in listing {
        // An entry the listing cannot describe is not a recording this could
        // open; the rest of the folder is still worth showing.
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let is_wav = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
        // A file that vanished between the listing and the read is skipped
        // for the same reason.
        if is_wav
            && path.is_file()
            && let Ok(found) = LibraryEntry::read(&path)
        {
            out.push(found);
        }
    }
    out.sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name()));
    Ok(out)
}

/// A file's name as the window shows it.
fn shown_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| Path::new(n).display().to_string(),
    )
}

/// A marker's name as the window shows it. The name is bytes -- the format
/// does not say which encoding -- so what is not UTF-8 is shown as `\xNN`
/// rather than replaced, and the bytes themselves are left as they are.
fn shown_label(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        for b in chunk.invalid() {
            out.push_str(&format!("\\x{b:02X}"));
        }
    }
    out
}

/// "44.1 kHz stereo, 16-bit".
fn format_line(info: &wavpcm::Info) -> String {
    let channels = match info.channels {
        1 => String::from("mono"),
        2 => String::from("stereo"),
        n => format!("{n} channels"),
    };
    let khz = format!("{:.3}", f64::from(info.sample_rate) / 1000.0);
    let khz = khz.trim_end_matches('0').trim_end_matches('.');
    format!("{khz} kHz {channels}, {}", info.format.label())
}

/// Seconds as `m:ss.t`, or `h:mm:ss.t` from an hour.
fn clock(seconds: f64) -> String {
    let tenths = (seconds.max(0.0) * 10.0).round() as u64;
    let part = |every: u64, of: u64| {
        tenths
            .checked_div(every)
            .unwrap_or(0)
            .checked_rem(of)
            .unwrap_or(0)
    };
    let hours = tenths.checked_div(36_000).unwrap_or(0);
    let (minutes, secs, tenth) = (part(600, 60), part(10, 60), part(1, 10));
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}.{tenth}")
    } else {
        format!("{minutes}:{secs:02}.{tenth}")
    }
}

/// Whether two paths name one file: compared as the filesystem resolves
/// them where it can, as written where it cannot.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// A take's file name: "Recording 2026-09-25 14-03-07", in UTC -- there is
/// no time zone an application can read -- and with dashes, since a colon is
/// not a name every filesystem a recording may be copied to allows.
fn take_name(now: SystemTime) -> String {
    let secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let date = guitk::date::Date::from_unix_utc(i64::try_from(secs).unwrap_or(i64::MAX));
    let (y, m, d) = date.ymd();
    let day = secs.checked_rem(86_400).unwrap_or(0);
    let (hh, mm, ss) = (
        day.checked_div(3_600).unwrap_or(0),
        day.checked_div(60)
            .unwrap_or(0)
            .checked_rem(60)
            .unwrap_or(0),
        day.checked_rem(60).unwrap_or(0),
    );
    format!("Recording {y:04}-{m:02}-{d:02} {hh:02}-{mm:02}-{ss:02}")
}

// ============================================================================
// The recording on screen
// ============================================================================

/// A marker's colour: fixed, not a theme role, for the reason [`MarkerList`]
/// gives -- a mark is the user's own, and a theme change must not recolour
/// the ones made before it.
const MARKER_COLORS: [u32; 4] = [0x0089_B4FA, 0x00FA_B387, 0x00F9_E2AF, 0x00A6_E3A1];

/// What a file looked like when it was read: enough to notice that another
/// program has written it since.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl Stamp {
    fn of(path: &Path) -> Result<Self, String> {
        let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self {
            len: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

/// A recording opened from disk: its bytes, its waveform, its markers, the
/// cursor and the stretch being kept.
pub struct OpenRecording {
    pub path: PathBuf,
    pub name: String,
    /// The file as it was read -- and as it was written, after markers are
    /// saved into it.
    bytes: Vec<u8>,
    pub info: wavpcm::Info,
    /// The waveform, in [`PEAK_COLUMNS`] stretches of the whole file.
    peaks: Vec<wavpcm::Peak>,
    /// The markers, in frame order.
    pub markers: Vec<wavpcm::Cue>,
    /// Whether the markers differ from the ones in the file.
    pub markers_changed: bool,
    /// The frame the cursor is on.
    pub cursor: u64,
    /// The stretch a "save the kept part" writes.
    pub kept: TrimRegion,
    /// The marker the keys act on, as its place in `markers`; every change
    /// to the list sets it again.
    pub chosen_marker: Option<usize>,
    stamp: Stamp,
}

impl OpenRecording {
    /// Read `path` whole.
    ///
    /// # Errors
    ///
    /// When it cannot be read, is over a gigabyte, or is not a WAV this reads.
    pub fn open(path: &Path) -> Result<Self, String> {
        let stamp = Stamp::of(path)?;
        let read = safeio::read_capped(path, MAX_RECORDING_BYTES)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if read.truncated {
            return Err(format!(
                "{} is larger than 1 GiB, more than this opens",
                shown_name(path)
            ));
        }
        let bytes = read.bytes;
        let fail = |e: wavpcm::WavError| format!("{}: {e}", shown_name(path));
        let info = wavpcm::parse_header(&bytes).map_err(fail)?;
        let peaks = wavpcm::peaks(&bytes, PEAK_COLUMNS).map_err(fail)?;
        let markers = wavpcm::cues(&bytes).map_err(fail)?;
        Ok(Self {
            path: path.to_path_buf(),
            name: shown_name(path),
            bytes,
            info,
            peaks,
            markers,
            markers_changed: false,
            cursor: 0,
            kept: TrimRegion::full(info.frames),
            chosen_marker: None,
            stamp,
        })
    }

    /// Seconds from the start to `frame`.
    pub fn seconds_at(&self, frame: u64) -> f64 {
        frame as f64 / f64::from(self.info.sample_rate.max(1))
    }

    /// Frames in `seconds`.
    fn frames_in(&self, seconds: f64) -> u64 {
        (seconds * f64::from(self.info.sample_rate))
            .round()
            .max(0.0) as u64
    }

    /// Move the cursor by `seconds`, stopping at the ends.
    pub fn nudge(&mut self, seconds: f64) {
        let by = self.frames_in(seconds.abs());
        self.cursor = if seconds < 0.0 {
            self.cursor.saturating_sub(by)
        } else {
            self.cursor.saturating_add(by)
        }
        .min(self.info.frames);
    }

    /// Put the cursor on the marker after it (or before), and choose that
    /// marker. Answers whether there was one.
    pub fn to_marker(&mut self, forward: bool) -> bool {
        let at = self.cursor;
        let found = if forward {
            self.markers.iter().position(|m| u64::from(m.frame) > at)
        } else {
            self.markers.iter().rposition(|m| u64::from(m.frame) < at)
        };
        let Some((i, frame)) = found.and_then(|i| Some((i, self.markers.get(i)?.frame))) else {
            return false;
        };
        self.cursor = u64::from(frame);
        self.chosen_marker = Some(i);
        true
    }

    /// Choose marker `i` and put the cursor on it.
    pub fn choose_marker(&mut self, i: usize) -> bool {
        let Some(frame) = self.markers.get(i).map(|m| m.frame) else {
            return false;
        };
        self.chosen_marker = Some(i);
        self.cursor = u64::from(frame);
        true
    }

    /// Put a marker at the cursor -- or choose the one already there -- and
    /// answer its place in the list.
    pub fn add_marker(&mut self) -> usize {
        let frame = u32::try_from(self.cursor).unwrap_or(u32::MAX);
        if let Some(i) = self.markers.iter().position(|m| m.frame == frame) {
            self.chosen_marker = Some(i);
            return i;
        }
        let number = (1..=self.markers.len().saturating_add(1))
            .find(|n| {
                let name = format!("Marker {n}").into_bytes();
                !self.markers.iter().any(|m| m.label == name)
            })
            .unwrap_or(1);
        let at = self.markers.partition_point(|m| m.frame < frame);
        self.markers.insert(
            at,
            wavpcm::Cue {
                frame,
                label: format!("Marker {number}").into_bytes(),
            },
        );
        self.markers_changed = true;
        self.chosen_marker = Some(at);
        at
    }

    /// Take the chosen marker away.
    pub fn remove_marker(&mut self) -> bool {
        let Some(i) = self.chosen_marker.filter(|i| *i < self.markers.len()) else {
            return false;
        };
        self.markers.remove(i);
        self.markers_changed = true;
        self.chosen_marker = None;
        true
    }

    /// Name the chosen marker.
    pub fn rename_marker(&mut self, label: &str) -> bool {
        let Some(marker) = self.chosen_marker.and_then(|i| self.markers.get_mut(i)) else {
            return false;
        };
        let label = label.as_bytes().to_vec();
        if marker.label != label {
            marker.label = label;
            self.markers_changed = true;
        }
        true
    }

    /// Move marker `i` to `frame`, keep the list in frame order and the
    /// marker chosen, and answer its new place.
    pub fn move_marker(&mut self, i: usize, frame: u64) -> Option<usize> {
        if i >= self.markers.len() {
            return None;
        }
        let mut marker = self.markers.remove(i);
        marker.frame = u32::try_from(frame.min(self.info.frames)).unwrap_or(u32::MAX);
        let at = self.markers.partition_point(|m| m.frame <= marker.frame);
        self.markers.insert(at, marker);
        self.markers_changed = true;
        self.chosen_marker = Some(at);
        self.cursor = frame.min(self.info.frames);
        Some(at)
    }

    /// The kept stretch starts at the cursor.
    pub fn keep_from_cursor(&mut self) {
        self.kept.set_start(self.cursor);
    }

    /// The kept stretch ends at the cursor.
    pub fn keep_to_cursor(&mut self) {
        self.kept.set_end(self.cursor);
    }

    /// Keep the whole recording.
    pub fn keep_all(&mut self) {
        self.kept = TrimRegion::full(self.info.frames);
    }

    /// Save the markers into the file -- unless another program has written
    /// it since it was read, in which case the file is left as it is and the
    /// answer says so.
    ///
    /// # Errors
    ///
    /// Why nothing was written.
    pub fn save_markers(&mut self) -> Result<(), String> {
        if Stamp::of(&self.path)? != self.stamp {
            return Err(format!(
                "{} has changed on disk since it was opened; open it again before saving markers into it",
                self.name
            ));
        }
        let marked = wavpcm::with_cues(&self.bytes, &self.markers)
            .map_err(|e| format!("{}: {e}", self.name))?;
        safeio::write_atomically(&self.path, &marked)
            .map_err(|e| format!("could not save the markers into {}: {e}", self.name))?;
        let len = u64::try_from(marked.len()).unwrap_or(u64::MAX);
        self.bytes = marked;
        self.markers_changed = false;
        // A stamp that cannot be read back is recorded as one no file has, so
        // the next save refuses rather than trusting a file it cannot see.
        self.stamp = Stamp::of(&self.path).unwrap_or(Stamp {
            len,
            modified: None,
        });
        Ok(())
    }

    /// Save the kept stretch, with the markers in it, as a file of its own at
    /// `to`; answer its size.
    ///
    /// # Errors
    ///
    /// Why nothing was written -- including `to` being this recording, whose
    /// every frame outside the stretch would be lost.
    pub fn save_kept(&self, to: &Path) -> Result<u64, String> {
        if same_file(to, &self.path) {
            return Err(format!(
                "That is {} itself, and keeping part of it there would lose the rest; choose another name",
                self.name
            ));
        }
        let fail = |e: wavpcm::WavError| format!("{}: {e}", self.name);
        let current = wavpcm::with_cues(&self.bytes, &self.markers).map_err(fail)?;
        let part =
            wavpcm::cut(&current, self.kept.start_frame, self.kept.end_frame).map_err(fail)?;
        safeio::write_atomically(to, &part)
            .map_err(|e| format!("could not write {}: {e}", to.display()))?;
        Ok(u64::try_from(part.len()).unwrap_or(u64::MAX))
    }
}

// ============================================================================
// Main application state
// ============================================================================

/// The keys this window answers, as a reader sees them.
const SHORTCUTS: &[(&str, &str)] = &[
    ("F1 / ?", "This list"),
    ("Ctrl+O", "Open a recording from anywhere"),
    ("F5", "Look in the recordings folder again"),
    ("Tab", "Recordings list, or the recording"),
    ("Up / Down", "Move through the recordings, or the markers"),
    ("Enter", "Open the chosen recording"),
    ("Left / Right", "Cursor back or on a tenth of a second"),
    ("Shift+Left / Shift+Right", "Cursor back or on a second"),
    ("Home / End", "Cursor to the start or the end"),
    (
        "Ctrl+Left / Ctrl+Right",
        "Cursor to the marker before or after",
    ),
    ("M", "Put a marker at the cursor"),
    ("F2", "Name the chosen marker"),
    ("Delete", "Take the chosen marker away"),
    ("[ / ]", "Keep from the cursor, or up to it"),
    ("Ctrl+A", "Keep the whole recording"),
    ("Ctrl+S", "Save the markers into the file"),
    ("Ctrl+Shift+S", "Save the kept part as a new file"),
    ("Space", "Record (or say why it cannot)"),
    ("P", "Play (or say why it cannot)"),
];

/// The longest marker name the field takes, in characters.
const LABEL_CAPACITY: usize = 120;

/// Which half of the window has the keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Library,
    Recording,
}

/// What a file picker is up for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerFor {
    Open,
    Folder,
    SaveKept,
}

/// What the pointer is dragging across the waveform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    Cursor,
    KeptStart,
    KeptEnd,
    Marker(usize),
}

/// What a press can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Help,
    HelpCard,
    Panel(Panel),
    OpenFile,
    ChooseFolder,
    Refresh,
    LibraryList,
    /// A recording in the list, by its place in it.
    LibraryRow(usize),
    Waveform,
    MarkerList,
    /// A marker in the list, by its place in it.
    MarkerRow(usize),
    AddMarker,
    RenameMarker,
    RemoveMarker,
    KeepFrom,
    KeepTo,
    KeepAll,
    SaveMarkers,
    SaveKept,
    Record,
    Pause,
    Stop,
    DropMarker,
    Play,
}

/// Top-level application state for the sound recorder.
pub struct SoundRecorderApp {
    /// Whether the shortcut card is up.
    pub show_help: bool,

    // -- The take ------------------------------------------------------------
    /// Where the take is: idle, recording, paused.
    pub state: RecordingState,
    pub preset: QualityPreset,
    pub sample_rate: SampleRate,
    /// Why the last Record or Play did nothing, if it did nothing.
    ///
    /// A reason rather than a flag: a press that changes nothing visible
    /// reads as a broken button and sends the user hunting the wrong fault.
    pub blocked_reason: Option<String>,
    /// Input devices. Empty: nothing here can enumerate one.
    pub input_devices: Vec<AudioInputDevice>,
    pub selected_device: usize,
    /// The take's samples.
    pub wav: WavFile,
    pub waveform: WaveformDisplay,
    pub vu_meter: VuMeter,
    pub timer: RecordingTimer,
    /// Markers dropped into the take as it runs.
    pub markers: MarkerList,
    pub noise_gate: NoiseGate,
    pub playback: PlaybackController,
    pub auto_save: AutoSave,

    // -- The recordings ------------------------------------------------------
    /// Where takes are saved and recordings listed: `~/Recordings` unless
    /// another folder was chosen. `None` with no home to put one in.
    pub recordings_dir: Option<PathBuf>,
    pub library: Vec<LibraryEntry>,
    /// Why the list is empty, or could not be read.
    pub library_note: Option<String>,
    /// The recording the keys are on in the list, by path: a rescan that
    /// reorders the list keeps it on the same file.
    pub chosen_entry: Option<PathBuf>,
    pub library_scroll: usize,
    /// The recording on screen.
    pub open: Option<OpenRecording>,
    /// A recording asked for while the open one has unsaved markers: asking
    /// again for the same one leaves them.
    leave_for: Option<PathBuf>,
    pub marker_scroll: usize,
    /// The chosen marker's name, while it is being written.
    pub rename: Option<TextInput>,
    /// What a copy or a cut in the name field took, for a paste.
    clipboard: String,
    pub picker: FilePicker,
    pub picker_for: PickerFor,
    pub panel: Panel,
    /// What the last action said.
    pub status_line: String,

    drag: Option<Drag>,
    /// Where the press that began the drag landed.
    press_x: f32,
    hover: Option<Target>,
    last_hits: Vec<(Target, Rect)>,
    wheel: wheel::Accumulator,
    pub window_width: f32,
    pub window_height: f32,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
}

impl SoundRecorderApp {
    /// A recorder with no folder and nothing open, which touches no file:
    /// what the tests build on. `main` uses [`Self::from_env`].
    pub fn new() -> Self {
        let preset = QualityPreset::Music;
        Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            show_help: false,
            state: RecordingState::Idle,
            preset,
            sample_rate: preset.sample_rate(),
            blocked_reason: None,
            // Empty. Nothing here can enumerate an audio device.
            input_devices: Vec::new(),
            selected_device: 0,
            wav: WavFile::from_preset(preset),
            waveform: WaveformDisplay::new(20.0, 120.0, 560.0, 100.0),
            vu_meter: VuMeter::new(20.0, 230.0, 560.0, 20.0),
            // `None`: nothing here can ask the filesystem how much room is
            // left. It was a flat 1_000_000_000.
            timer: RecordingTimer::new(None, preset.bytes_per_second()),
            markers: MarkerList::new(),
            noise_gate: NoiseGate::new(0.02),
            playback: PlaybackController::new(),
            auto_save: AutoSave::new(60),
            recordings_dir: None,
            library: Vec::new(),
            library_note: None,
            chosen_entry: None,
            library_scroll: 0,
            open: None,
            leave_for: None,
            marker_scroll: 0,
            rename: None,
            clipboard: String::new(),
            picker: FilePicker::default(),
            picker_for: PickerFor::Open,
            panel: Panel::Library,
            status_line: String::new(),
            drag: None,
            press_x: 0.0,
            hover: None,
            last_hits: Vec::new(),
            wheel: wheel::Accumulator::default(),
            window_width: 980.0,
            window_height: 640.0,
        }
    }

    /// The recorder `main` opens: `~/Recordings`, listed.
    pub fn from_env() -> Self {
        let mut app = Self::new();
        app.recordings_dir =
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Recordings"));
        app.rescan();
        app
    }

    /// A recorder with input devices, for tests.
    ///
    /// `#[cfg(test)]`. Most of the take's tests are about the transport, the
    /// timer and markers, and need *an* input to exist rather than a specific
    /// invented one. Before 2026-09-15 they got it from `new`, which is
    /// exactly the problem: the device list production could reach was the
    /// device list that shipped.
    #[cfg(test)]
    fn with_mock_input() -> Self {
        let mut app = Self::new();
        app.input_devices = AudioInputDevice::mock_devices();
        app
    }

    // -----------------------------------------------------------------------
    // The take
    // -----------------------------------------------------------------------

    /// Transition to a new recording state if the transition is valid.
    /// Returns true if the transition was performed.
    pub fn transition_to(&mut self, target: RecordingState) -> bool {
        if !self.state.can_transition_to(target) {
            return false;
        }

        // Refuse to start a take there is no input for.
        //
        // `process_samples` is the door audio comes in through, and nothing in
        // production calls it -- there is no capture device to call it from.
        // Before 2026-09-15 the take started anyway: the clock climbed through
        // minutes while zero audio existed, the auto-save counted saves of
        // nothing, and the state read "Recording". A person recording an
        // interview would have watched all three and had nothing, and the
        // event is not repeatable.
        if target == RecordingState::Recording && self.current_device().is_none() {
            self.blocked_reason = Some(String::from(CANNOT_RECORD));
            return false;
        }

        if target == RecordingState::Recording && self.state == RecordingState::Idle {
            // A new take: everything from the last one goes.
            self.wav = WavFile::from_preset(self.preset);
            self.waveform.clear();
            self.vu_meter.reset();
            self.timer.reset();
            self.markers.clear();
            self.noise_gate.reset();
            self.auto_save.reset_timer();
        }
        self.state = target;
        true
    }

    /// End the take: save it as a new file in the recordings folder, open it
    /// to be marked and trimmed, and go back to idle. The take is kept in
    /// memory if the save fails, so a second Stop can try again.
    pub fn stop_take(&mut self) -> bool {
        if !matches!(
            self.state,
            RecordingState::Recording | RecordingState::Paused
        ) {
            return false;
        }
        self.transition_to(RecordingState::Stopped);
        match self.save_take() {
            Ok(path) => {
                self.transition_to(RecordingState::Idle);
                self.rescan();
                self.open_now(&path);
                self.status_line = format!("Saved the take as {}", shown_name(&path));
            }
            Err(why) => {
                self.status_line =
                    format!("The take is not saved: {why}. Press Stop to try again.");
                self.state = RecordingState::Paused;
            }
        }
        true
    }

    /// Write the take as a new file in the recordings folder, named for the
    /// moment, never over another; answer where it went.
    fn save_take(&self) -> Result<PathBuf, String> {
        let dir = self
            .recordings_dir
            .clone()
            .ok_or_else(|| String::from("there is no recordings folder (no home folder is set)"))?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let bytes = self.wav.to_wav(&self.markers).map_err(|e| e.to_string())?;
        let stem = take_name(SystemTime::now());
        for n in 1..=999_u32 {
            let name = if n == 1 {
                format!("{stem}.wav")
            } else {
                format!("{stem} ({n}).wav")
            };
            let path = dir.join(name);
            match safeio::write_new_atomically(&path, &bytes) {
                Ok(()) => return Ok(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(format!("{}: {e}", path.display())),
            }
        }
        Err(format!(
            "every name for this take in {} is taken",
            dir.display()
        ))
    }

    /// Set the quality preset and update related settings.
    pub fn set_preset(&mut self, preset: QualityPreset) {
        self.preset = preset;
        self.sample_rate = preset.sample_rate();
        self.timer.set_bytes_per_second(preset.bytes_per_second());
    }

    /// Select an input device by index.
    pub fn select_device(&mut self, index: usize) -> bool {
        if index < self.input_devices.len() {
            self.selected_device = index;
            true
        } else {
            false
        }
    }

    /// Get the currently selected input device, if any.
    pub fn current_device(&self) -> Option<&AudioInputDevice> {
        self.input_devices.get(self.selected_device)
    }

    /// Process incoming audio samples during recording.
    pub fn process_samples(&mut self, samples: &[i16]) {
        if self.state != RecordingState::Recording {
            return;
        }
        let mut processed = samples.to_vec();
        self.noise_gate.process(&mut processed);
        self.wav.push_samples(&processed);
        let amplitude = WaveformDisplay::amplitude_from_samples(samples);
        self.waveform.push_amplitude(amplitude);
        self.vu_meter.update(amplitude);
    }

    /// Tick the application timers by delta milliseconds.
    pub fn tick(&mut self, delta_ms: u64) {
        if self.state == RecordingState::Recording {
            self.timer.tick(delta_ms);
        }
    }

    /// Check if auto-save should trigger. Returns true when it fires.
    pub fn check_auto_save(&mut self, delta_ms: u64) -> bool {
        if self.state == RecordingState::Recording {
            self.auto_save.tick(delta_ms)
        } else {
            false
        }
    }

    /// Add a marker at the current recording position.
    pub fn add_marker(&mut self, label: String) -> Option<u32> {
        if self.state == RecordingState::Recording || self.state == RecordingState::Paused {
            let frame = self.wav.frame_count() as u64;
            Some(self.markers.add(frame, label))
        } else {
            None
        }
    }

    /// Record: start, pause or carry on -- or, with nothing to record from,
    /// say so.
    fn record_key(&mut self) -> bool {
        match self.state {
            RecordingState::Idle | RecordingState::Stopped => {
                self.transition_to(RecordingState::Recording);
            }
            RecordingState::Recording => {
                self.transition_to(RecordingState::Paused);
            }
            RecordingState::Paused => {
                self.transition_to(RecordingState::Recording);
            }
        }
        true
    }

    /// Play: there is nothing an application can play through.
    fn play(&mut self) {
        self.blocked_reason = Some(String::from(CANNOT_PLAY));
    }

    // -----------------------------------------------------------------------
    // The recordings
    // -----------------------------------------------------------------------

    /// List the recordings folder again.
    pub fn rescan(&mut self) {
        let Some(dir) = self.recordings_dir.clone() else {
            self.library.clear();
            self.library_note = Some(String::from(
                "There is no recordings folder: no home folder is set.",
            ));
            return;
        };
        match scan(&dir) {
            Ok(found) => {
                self.library_note = found.is_empty().then(|| {
                    String::from("No recordings here yet. Ctrl+O opens one from anywhere.")
                });
                self.library = found;
            }
            Err(_) if !dir.exists() => {
                self.library.clear();
                self.library_note = Some(format!(
                    "{} does not exist yet; the first take will make it.",
                    dir.display()
                ));
            }
            Err(why) => {
                self.library.clear();
                self.library_note = Some(format!("Cannot list the folder: {why}"));
            }
        }
        if let Some(chosen) = &self.chosen_entry
            && !self.library.iter().any(|e| &e.path == chosen)
        {
            self.chosen_entry = None;
        }
        self.clamp_scrolls();
    }

    /// Where the chosen recording sits in the list.
    fn chosen_index(&self) -> Option<usize> {
        let chosen = self.chosen_entry.as_ref()?;
        self.library.iter().position(|e| &e.path == chosen)
    }

    /// Move the choice in the list by `delta`, stopping at the ends.
    fn move_entry(&mut self, delta: isize) {
        if self.library.is_empty() {
            self.chosen_entry = None;
            return;
        }
        let last = (self.library.len() as isize).saturating_sub(1);
        let next = match self.chosen_index() {
            Some(i) => (i as isize).saturating_add(delta).clamp(0, last),
            None if delta < 0 => last,
            None => 0,
        };
        self.chosen_entry = self
            .library
            .get(next.unsigned_abs())
            .map(|e| e.path.clone());
    }

    /// Open `path` -- unless the recording on screen has markers not saved,
    /// in which case the first request says so and a second one leaves them.
    pub fn open_path(&mut self, path: &Path) -> bool {
        if let Some(open) = &self.open
            && open.markers_changed
            && open.path != path
            && self.leave_for.as_deref() != Some(path)
        {
            self.status_line = format!(
                "The markers in {} are not saved. Ctrl+S saves them; open {} again to leave them.",
                open.name,
                shown_name(path)
            );
            self.leave_for = Some(path.to_path_buf());
            return false;
        }
        self.open_now(path)
    }

    /// Open `path`, replacing whatever is on screen.
    fn open_now(&mut self, path: &Path) -> bool {
        self.leave_for = None;
        match OpenRecording::open(path) {
            Ok(rec) => {
                self.status_line = format!(
                    "Opened {}: {}, {}",
                    rec.name,
                    clock(rec.info.seconds()),
                    format_line(&rec.info)
                );
                self.open = Some(rec);
                self.rename = None;
                self.marker_scroll = 0;
                self.panel = Panel::Recording;
                true
            }
            Err(why) => {
                self.status_line = why;
                false
            }
        }
    }

    /// Save the open recording's markers into it, and say how that went.
    pub fn save_markers(&mut self) -> bool {
        let Some(open) = self.open.as_mut() else {
            self.status_line = String::from("Nothing is open to save markers into");
            return false;
        };
        match open.save_markers() {
            Ok(()) => {
                self.status_line = format!(
                    "Saved {} marker{} into {}",
                    open.markers.len(),
                    if open.markers.len() == 1 { "" } else { "s" },
                    open.name
                );
                let path = open.path.clone();
                // The list shows sizes; this one has changed.
                if let Ok(entry) = LibraryEntry::read(&path)
                    && let Some(slot) = self.library.iter_mut().find(|e| e.path == path)
                {
                    *slot = entry;
                }
                true
            }
            Err(why) => {
                self.status_line = why;
                false
            }
        }
    }

    /// Ask where the kept part should go.
    fn ask_where_to_keep(&mut self) {
        let Some(open) = &self.open else {
            self.status_line = String::from("Nothing is open to keep a part of");
            return;
        };
        let stem = open.path.file_stem().map_or_else(
            || String::from("recording"),
            |s| Path::new(s).display().to_string(),
        );
        let folder = open.path.parent().map(Path::to_path_buf);
        let mut dialog = FileDialog::save().with_filename(format!("{stem} (part).wav"));
        if let Some(folder) = folder {
            dialog = dialog.with_initial_path(folder);
        }
        self.picker_for = PickerFor::SaveKept;
        self.picker.put_up(dialog, true);
    }

    /// Put a file dialog up for `purpose`.
    fn open_picker(&mut self, purpose: PickerFor) {
        self.picker_for = purpose;
        match purpose {
            PickerFor::Open => self.picker.open_to_read(),
            PickerFor::Folder => self.picker.put_up(
                FileDialog::select_folder().with_initial_path(
                    self.recordings_dir
                        .clone()
                        .filter(|d| d.exists())
                        .unwrap_or_else(FilePicker::default_start),
                ),
                false,
            ),
            PickerFor::SaveKept => self.ask_where_to_keep(),
        }
    }

    /// What the file dialog chose.
    fn picked(&mut self, path: &Path) {
        match self.picker_for {
            PickerFor::Open => {
                self.open_path(path);
            }
            PickerFor::Folder => {
                self.recordings_dir = Some(path.to_path_buf());
                self.chosen_entry = None;
                self.library_scroll = 0;
                self.rescan();
                self.status_line = format!(
                    "Listing {}; takes are saved there too, until the window closes",
                    path.display()
                );
            }
            PickerFor::SaveKept => {
                let Some(open) = &self.open else { return };
                self.status_line = match open.save_kept(path) {
                    Ok(size) => {
                        let kept = open.seconds_at(open.kept.length_frames());
                        format!(
                            "Saved {} of {} as {} ({})",
                            clock(kept),
                            open.name,
                            shown_name(path),
                            guitk::bytes::iec(size)
                        )
                    }
                    Err(why) => why,
                };
                self.rescan();
            }
        }
    }

    // -----------------------------------------------------------------------
    // Keys
    // -----------------------------------------------------------------------

    /// Route one event.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        // The dialog takes input first while it is up, or a name typed into
        // it would reach the window behind.
        match self
            .picker
            .handle(event, self.window_width, self.window_height)
        {
            Picked::Chose(path) => {
                self.picked(&path);
                return EventResult::Consumed;
            }
            Picked::Handled | Picked::Cancelled => return EventResult::Consumed,
            Picked::Ignored => {}
        }
        match event {
            Event::Resize { width, height } => {
                self.window_width = *width as f32;
                self.window_height = *height as f32;
                self.clamp_scrolls();
                EventResult::Consumed
            }
            Event::Tick { elapsed_ms } => {
                // `elapsed_ms`, never the interval asked for: a busy frame
                // delivers one long tick, and a recorder that counted ticks
                // would report a duration that drifts from the audio.
                self.tick(*elapsed_ms);
                let saved = self.check_auto_save(*elapsed_ms);
                if self.state == RecordingState::Recording || saved {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::Key(key) if key.pressed => {
                let before = (self.chosen_entry.clone(), self.chosen_marker());
                let result = self.handle_key(key);
                self.follow_choice(
                    self.chosen_entry != before.0 || matches!(key.key, Key::Up | Key::Down),
                    self.chosen_marker() != before.1,
                );
                result
            }
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            _ => EventResult::Ignored,
        }
    }

    /// The chosen marker, if a recording is open.
    fn chosen_marker(&self) -> Option<usize> {
        self.open.as_ref().and_then(|o| o.chosen_marker)
    }

    /// Keyboard control.
    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if self.rename.is_some() {
            return self.rename_key(key);
        }
        if key.key == Key::F1 || (key.key == Key::Slash && key.modifiers.shift) {
            self.show_help = !self.show_help;
            return EventResult::Consumed;
        }
        if self.show_help {
            // Modal: Space would start a take from behind the card.
            if matches!(key.key, Key::Escape | Key::Enter) {
                self.show_help = false;
            }
            return EventResult::Consumed;
        }
        let ctrl = key.modifiers.ctrl;
        let shift = key.modifiers.shift;
        match key.key {
            Key::O if ctrl => self.open_picker(PickerFor::Open),
            Key::S if ctrl && shift => self.open_picker(PickerFor::SaveKept),
            Key::S if ctrl => {
                self.save_markers();
            }
            Key::A if ctrl => self.with_open(OpenRecording::keep_all),
            Key::F5 => {
                self.rescan();
                self.status_line = String::from("Looked in the folder again");
            }
            Key::Tab => {
                self.panel = match self.panel {
                    Panel::Library => Panel::Recording,
                    Panel::Recording => Panel::Library,
                };
            }
            Key::Up | Key::Down if self.panel == Panel::Library => {
                self.move_entry(if key.key == Key::Down { 1 } else { -1 });
            }
            Key::Up | Key::Down => {
                let down = key.key == Key::Down;
                if let Some(open) = self.open.as_mut() {
                    let last = open.markers.len().checked_sub(1);
                    let next = match (open.chosen_marker, last) {
                        (_, None) => None,
                        (Some(i), Some(last)) if down => Some(i.saturating_add(1).min(last)),
                        (Some(i), Some(_)) => Some(i.saturating_sub(1)),
                        (None, Some(last)) => Some(if down { 0 } else { last }),
                    };
                    if let Some(i) = next {
                        open.choose_marker(i);
                    }
                }
            }
            Key::Enter if self.panel == Panel::Library => {
                let Some(path) = self.chosen_entry.clone() else {
                    return EventResult::Ignored;
                };
                self.open_path(&path);
            }
            Key::Left | Key::Right => {
                let forward = key.key == Key::Right;
                if ctrl {
                    self.with_open(|o| {
                        o.to_marker(forward);
                    });
                } else {
                    let step = if shift { 1.0 } else { 0.1 };
                    self.with_open(|o| o.nudge(if forward { step } else { -step }));
                }
            }
            Key::Home => self.with_open(|o| o.cursor = 0),
            Key::End => self.with_open(|o| o.cursor = o.info.frames),
            Key::M if self.state != RecordingState::Idle => {
                let n = self.markers.len().saturating_add(1);
                self.add_marker(format!("Marker {n}"));
            }
            Key::M => self.with_open(|o| {
                o.add_marker();
            }),
            Key::F2 => self.start_rename(),
            Key::Delete => self.with_open(|o| {
                o.remove_marker();
            }),
            Key::LeftBracket => self.with_open(OpenRecording::keep_from_cursor),
            Key::RightBracket => self.with_open(OpenRecording::keep_to_cursor),
            Key::Space => {
                self.record_key();
            }
            Key::S if !ctrl && self.state != RecordingState::Idle => {
                self.stop_take();
            }
            Key::P => self.play(),
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// Do `act` to the open recording -- or say there is none.
    fn with_open(&mut self, act: impl FnOnce(&mut OpenRecording)) {
        match self.open.as_mut() {
            Some(open) => act(open),
            None => {
                self.status_line = String::from("Nothing is open: choose a recording, or Ctrl+O");
            }
        }
    }

    /// Start writing the chosen marker's name.
    fn start_rename(&mut self) {
        let Some(label) = self
            .open
            .as_ref()
            .and_then(|o| o.markers.get(o.chosen_marker?))
            .map(|m| shown_label(&m.label))
        else {
            self.status_line = String::from("Choose a marker to name first");
            return;
        };
        let mut input = TextInput::new();
        input.set_text(&label);
        input.select_all();
        self.rename = Some(input);
        self.panel = Panel::Recording;
    }

    /// A key while a marker's name is being written: Enter keeps it, Escape
    /// leaves the old one.
    fn rename_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Enter => self.commit_rename(),
            Key::Escape => self.rename = None,
            _ => {
                if let Some(input) = self.rename.as_mut()
                    && let Some(copied) =
                        textline::apply_key(input, key, LABEL_CAPACITY, &self.clipboard, 13.0)
                            .copied
                {
                    self.clipboard = copied;
                }
            }
        }
        EventResult::Consumed
    }

    /// Keep the name being written.
    fn commit_rename(&mut self) {
        if let Some(input) = self.rename.take() {
            let name = input.text().trim().to_owned();
            self.with_open(|o| {
                o.rename_marker(&name);
            });
        }
    }

    // -----------------------------------------------------------------------
    // Scrolling
    // -----------------------------------------------------------------------

    /// The recordings list's rows, and how many whole ones fit.
    fn library_pane(&self) -> (Rect, usize) {
        let top = self.content_top() + 96.0;
        let bottom = self.window_height - STATUS_H;
        let pane = Rect::new(0.0, top, SIDEBAR_W, (bottom - top).max(0.0));
        (pane, ((pane.h / LIB_ROW_H).floor().max(1.0)) as usize)
    }

    /// The markers list's rows, and how many whole ones fit.
    fn marker_pane(&self) -> (Rect, usize) {
        let x = SIDEBAR_W;
        let top = self.content_top() + 56.0 + WAVE_H + 128.0;
        let bottom = self.window_height - STATUS_H - TRANSPORT_H;
        let pane = Rect::new(
            x,
            top,
            (self.window_width - x).max(0.0),
            (bottom - top).max(0.0),
        );
        (pane, ((pane.h / MARKER_ROW_H).floor().max(1.0)) as usize)
    }

    /// Scroll a list whose choice the keys just moved, so the choice is on
    /// screen -- only then, so a list the wheel moved stays where it went.
    fn follow_choice(&mut self, entry_moved: bool, marker_moved: bool) {
        if entry_moved && let Some(at) = self.chosen_index() {
            let (_, rows) = self.library_pane();
            self.library_scroll = scrolled_to(self.library_scroll, at, rows);
        }
        if marker_moved && let Some(at) = self.chosen_marker() {
            let (_, rows) = self.marker_pane();
            self.marker_scroll = scrolled_to(self.marker_scroll, at, rows);
        }
        self.clamp_scrolls();
    }

    /// Keep each list's scroll inside the list.
    fn clamp_scrolls(&mut self) {
        let (_, rows) = self.library_pane();
        self.library_scroll = self
            .library_scroll
            .min(self.library.len().saturating_sub(rows));
        let (_, rows) = self.marker_pane();
        let markers = self.open.as_ref().map_or(0, |o| o.markers.len());
        self.marker_scroll = self.marker_scroll.min(markers.saturating_sub(rows));
    }

    /// The wheel over a list.
    fn wheel_at(&mut self, x: f32, y: f32, dy: f32) -> EventResult {
        let library = match self.target_at(x, y) {
            Some(Target::LibraryList | Target::LibraryRow(_)) => true,
            Some(Target::MarkerList | Target::MarkerRow(_)) => false,
            _ => return EventResult::Ignored,
        };
        let rows = self.wheel.rows(dy);
        let (now, count, visible) = if library {
            (
                self.library_scroll,
                self.library.len(),
                self.library_pane().1,
            )
        } else {
            (
                self.marker_scroll,
                self.open.as_ref().map_or(0, |o| o.markers.len()),
                self.marker_pane().1,
            )
        };
        let next = if rows < 0 {
            now.saturating_sub(rows.unsigned_abs())
        } else {
            now.saturating_add(rows.unsigned_abs())
        }
        .min(count.saturating_sub(visible));
        if next == now {
            return EventResult::Ignored;
        }
        if library {
            self.library_scroll = next;
        } else {
            self.marker_scroll = next;
        }
        EventResult::Consumed
    }
}

/// The scroll that shows row `at` in a pane of `rows` rows, moving `scroll`
/// as little as it can.
fn scrolled_to(scroll: usize, at: usize, rows: usize) -> usize {
    if at < scroll {
        at
    } else if at >= scroll.saturating_add(rows) {
        at.saturating_add(1).saturating_sub(rows)
    } else {
        scroll
    }
}

// ============================================================================
// Drawing
// ============================================================================

const TITLE_H: f32 = 40.0;
const NOTICE_H: f32 = 40.0;
const SIDEBAR_W: f32 = 300.0;
const STATUS_H: f32 = 24.0;
const LIB_ROW_H: f32 = 40.0;
const MARKER_ROW_H: f32 = 26.0;
const WAVE_H: f32 = 160.0;
const TRANSPORT_H: f32 = 56.0;
/// How near a press must land to a marker or a handle to take it.
const HANDLE_REACH: f32 = 5.0;
/// How far a marker must be dragged before it moves: a press on a marker
/// chooses it, and a hand that shakes must not move it as well.
const DRAG_SLOP: f32 = 3.0;

/// A marker's colour, by its place in the list.
fn marker_color(i: usize) -> Color {
    let slot = i.checked_rem(MARKER_COLORS.len()).unwrap_or(0);
    Color::from_hex(MARKER_COLORS.get(slot).copied().unwrap_or(0x0089_B4FA))
}

/// Where `frame` of `frames` falls across `r`.
fn frame_x(r: Rect, frame: u64, frames: u64) -> f32 {
    if frames == 0 {
        return r.x;
    }
    r.x + (frame as f64 / frames as f64) as f32 * r.w
}

/// The frame under `x` across `r`.
fn frame_at(r: Rect, x: f32, frames: u64) -> u64 {
    if r.w <= 0.0 {
        return 0;
    }
    let f = f64::from(((x - r.x) / r.w).clamp(0.0, 1.0)) * frames as f64;
    (f.round() as u64).min(frames)
}

impl SoundRecorderApp {
    /// Where the window's content starts, under the title and the notice.
    fn content_top(&self) -> f32 {
        if self.input_devices.is_empty() {
            TITLE_H + NOTICE_H
        } else {
            TITLE_H
        }
    }

    /// The waveform's box.
    fn wave_rect(&self) -> Rect {
        let x = SIDEBAR_W + 16.0;
        Rect::new(
            x,
            self.content_top() + 56.0,
            (self.window_width - x - 16.0).max(1.0),
            WAVE_H,
        )
    }

    /// The whole window, and where every control in it is.
    fn frame(&self) -> Frame<Target> {
        let (w, h) = (self.window_width, self.window_height);
        let mut f = Frame::new(w, h);
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });
        self.render_title(&mut f);
        self.render_library(&mut f);
        self.render_recording(&mut f);
        self.render_transport(&mut f);
        self.render_status(&mut f);
        if self.picker.is_open() {
            f.extend(self.picker.render(&self.palette, w, h));
        }
        if self.show_help {
            guitk::shortcut::render_card(
                &mut f,
                &self.palette,
                (w, h),
                TITLE_H,
                SHORTCUTS,
                "F1 closes this",
            );
            f.hit(Target::HelpCard, Rect::new(0.0, 0.0, w, h));
        }
        f
    }

    /// A button: a press on it does `target`; a disabled one takes no press.
    fn button(
        &self,
        f: &mut Frame<Target>,
        rect: Rect,
        label: &str,
        target: Target,
        enabled: bool,
    ) {
        let lit = enabled && self.hover == Some(target);
        f.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: if lit {
                self.palette.surface2
            } else {
                self.palette.surface1
            },
            corner_radii: CornerRadii::all(6.0),
        });
        f.push(RenderCommand::Text {
            x: guitk::text::center_x(label, rect.x + rect.w / 2.0, 12.0, FontWeightHint::Regular)
                .max(rect.x + 4.0),
            y: rect.y + (rect.h - 12.0) / 2.0,
            text: label.to_string(),
            font_size: 12.0,
            color: if enabled {
                self.palette.text
            } else {
                self.palette.overlay0
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some((rect.w - 8.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        if enabled {
            f.hit(target, rect);
        }
    }

    /// Buttons left to right from `x`, each as wide as it says.
    fn buttons_from(
        &self,
        f: &mut Frame<Target>,
        x: f32,
        y: f32,
        buttons: &[(&str, f32, Target, bool)],
    ) {
        let mut at = x;
        for (label, width, target, enabled) in buttons {
            self.button(f, Rect::new(at, y, *width, 28.0), label, *target, *enabled);
            at += width + 6.0;
        }
    }

    /// One line of text.
    fn text(
        &self,
        f: &mut Frame<Target>,
        x: f32,
        y: f32,
        text: String,
        size: f32,
        color: Color,
        max: f32,
    ) {
        f.push(RenderCommand::Text {
            x,
            y,
            text,
            font_size: size,
            color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max.max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// A small heading.
    fn heading(&self, f: &mut Frame<Target>, x: f32, y: f32, text: String, max: f32) {
        f.push(RenderCommand::Text {
            x,
            y,
            text,
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max.max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// The border that says which half has the keys; a press in a half gives
    /// it them.
    fn panel_focus(&self, f: &mut Frame<Target>, panel: Panel, rect: Rect) {
        f.hit(Target::Panel(panel), rect);
        if self.panel == panel {
            f.push(RenderCommand::StrokeRect {
                x: rect.x + 1.0,
                y: rect.y + 1.0,
                width: (rect.w - 2.0).max(0.0),
                height: (rect.h - 2.0).max(0.0),
                color: self.palette.blue,
                line_width: 2.0,
                corner_radii: CornerRadii::ZERO,
            });
        }
    }

    fn render_title(&self, f: &mut Frame<Target>) {
        let w = self.window_width;
        self.palette
            .push_surface(f, 0.0, 0.0, w, TITLE_H, 0.0, Surface::Card);
        f.push(RenderCommand::Text {
            x: 16.0,
            y: 11.0,
            text: String::from("Sound Recorder"),
            font_size: 16.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Clip,
        });
        if self.state != RecordingState::Idle {
            f.push(RenderCommand::FillRect {
                x: 190.0,
                y: 15.0,
                width: 10.0,
                height: 10.0,
                color: self.state.color(&self.palette),
                corner_radii: CornerRadii::all(5.0),
            });
            self.text(
                f,
                206.0,
                12.0,
                self.state.label().to_owned(),
                14.0,
                self.palette.ink(self.state.color(&self.palette)),
                160.0,
            );
        }
        self.button(
            f,
            Rect::new(w - 44.0, 6.0, 32.0, 28.0),
            "?",
            Target::Help,
            true,
        );
        // Why there is no take, above everything else, and before Record is
        // pressed: a message that appears only after the press has already let
        // the user believe the take began.
        if self.input_devices.is_empty() {
            for (i, line) in NO_AUDIO_LINES.iter().enumerate() {
                let first = i == 0;
                f.push(RenderCommand::Text {
                    x: 16.0,
                    y: TITLE_H + 4.0 + i as f32 * 17.0,
                    text: (*line).to_string(),
                    color: if first {
                        self.palette.ink(self.palette.yellow)
                    } else {
                        self.palette.subtext0
                    },
                    font_size: if first { 13.0 } else { 11.0 },
                    font_weight: if first {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    max_width: Some((w - 32.0).max(0.0)),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }
    }

    fn render_library(&self, f: &mut Frame<Target>) {
        let top = self.content_top();
        let bottom = self.window_height - STATUS_H;
        let panel = Rect::new(0.0, top, SIDEBAR_W, (bottom - top).max(0.0));
        self.palette
            .push_surface(f, panel.x, panel.y, panel.w, panel.h, 0.0, Surface::Sidebar);
        self.panel_focus(f, Panel::Library, panel);
        self.heading(
            f,
            16.0,
            top + 12.0,
            String::from("RECORDINGS"),
            SIDEBAR_W - 32.0,
        );
        let folder = self
            .recordings_dir
            .as_ref()
            .map_or_else(|| String::from("No folder"), |d| d.display().to_string());
        self.text(
            f,
            16.0,
            top + 30.0,
            folder,
            11.0,
            self.palette.subtext1,
            SIDEBAR_W - 32.0,
        );
        self.buttons_from(
            f,
            16.0,
            top + 54.0,
            &[
                ("Open\u{2026}", 84.0, Target::OpenFile, true),
                ("Folder\u{2026}", 84.0, Target::ChooseFolder, true),
                (
                    "Refresh",
                    84.0,
                    Target::Refresh,
                    self.recordings_dir.is_some(),
                ),
            ],
        );
        let (pane, rows) = self.library_pane();
        f.hit(Target::LibraryList, pane);
        if let Some(note) = &self.library_note {
            self.text(
                f,
                16.0,
                pane.y + 8.0,
                note.clone(),
                11.0,
                self.palette.subtext0,
                SIDEBAR_W - 32.0,
            );
        }
        let chosen = self.chosen_index();
        let open = self.open.as_ref().map(|o| o.path.as_path());
        for (shown, (i, entry)) in self
            .library
            .iter()
            .enumerate()
            .skip(self.library_scroll)
            .take(rows)
            .enumerate()
        {
            let y = pane.y + shown as f32 * LIB_ROW_H;
            let row = Rect::new(4.0, y, SIDEBAR_W - 8.0, LIB_ROW_H - 2.0);
            if chosen == Some(i) || self.hover == Some(Target::LibraryRow(i)) {
                self.palette.push_surface(
                    f,
                    row.x,
                    row.y,
                    row.w,
                    row.h,
                    4.0,
                    if chosen == Some(i) {
                        Surface::Selected
                    } else {
                        Surface::Card
                    },
                );
            }
            let is_open = open == Some(entry.path.as_path());
            f.push(RenderCommand::Text {
                x: 14.0,
                y: y + 4.0,
                text: entry.name.clone(),
                font_size: 13.0,
                color: self.palette.text,
                font_weight: if is_open {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(SIDEBAR_W - 28.0),
                overflow: TextOverflow::Ellipsis,
            });
            self.text(
                f,
                14.0,
                y + 21.0,
                entry.summary(),
                10.0,
                if entry.detail.is_ok() {
                    self.palette.subtext0
                } else {
                    self.palette.ink(self.palette.red)
                },
                SIDEBAR_W - 28.0,
            );
            f.hit(Target::LibraryRow(i), row);
        }
    }

    fn render_recording(&self, f: &mut Frame<Target>) {
        let top = self.content_top();
        let x = SIDEBAR_W;
        let w = (self.window_width - x).max(0.0);
        let bottom = self.window_height - STATUS_H - TRANSPORT_H;
        let panel = Rect::new(x, top, w, (bottom - top).max(0.0));
        self.panel_focus(f, Panel::Recording, panel);
        let Some(open) = &self.open else {
            self.text(
                f,
                x + 24.0,
                top + 24.0,
                String::from("Nothing is open."),
                15.0,
                self.palette.text,
                w - 48.0,
            );
            self.text(
                f,
                x + 24.0,
                top + 46.0,
                String::from(
                    "Choose a recording on the left, or press Ctrl+O to open one from anywhere.",
                ),
                12.0,
                self.palette.subtext0,
                w - 48.0,
            );
            return;
        };
        f.push(RenderCommand::Text {
            x: x + 16.0,
            y: top + 10.0,
            text: open.name.clone(),
            font_size: 16.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some((w - 200.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        self.text(
            f,
            x + 16.0,
            top + 32.0,
            format!(
                "{}  \u{00B7}  {}",
                clock(open.info.seconds()),
                format_line(&open.info)
            ),
            12.0,
            self.palette.subtext0,
            w - 200.0,
        );
        if open.markers_changed {
            let note = "Markers not saved";
            f.push(RenderCommand::Text {
                x: guitk::text::right_x(note, x + w - 16.0, 12.0, FontWeightHint::Bold),
                y: top + 14.0,
                text: note.to_owned(),
                font_size: 12.0,
                color: self.palette.ink(self.palette.yellow),
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
        let r = self.wave_rect();
        self.render_waveform(f, open, r);

        let kept = &open.kept;
        self.text(
            f,
            r.x,
            r.y + r.h + 8.0,
            format!(
                "Cursor {}    Kept {} \u{2013} {} ({})",
                clock(open.seconds_at(open.cursor)),
                clock(open.seconds_at(kept.start_frame)),
                clock(open.seconds_at(kept.end_frame)),
                clock(open.seconds_at(kept.length_frames())),
            ),
            12.0,
            self.palette.subtext1,
            r.w,
        );
        let chosen = open.chosen_marker.is_some();
        self.buttons_from(
            f,
            r.x,
            r.y + r.h + 32.0,
            &[
                ("Add marker", 96.0, Target::AddMarker, true),
                ("Name", 64.0, Target::RenameMarker, chosen),
                ("Remove", 72.0, Target::RemoveMarker, chosen),
                ("Start here", 88.0, Target::KeepFrom, true),
                ("End here", 80.0, Target::KeepTo, true),
                ("Keep all", 76.0, Target::KeepAll, !kept.is_full()),
            ],
        );
        self.buttons_from(
            f,
            r.x,
            r.y + r.h + 66.0,
            &[
                (
                    "Save markers",
                    112.0,
                    Target::SaveMarkers,
                    open.markers_changed,
                ),
                (
                    "Save kept part\u{2026}",
                    128.0,
                    Target::SaveKept,
                    kept.length_frames() > 0,
                ),
            ],
        );
        self.render_markers(f, open);
    }

    fn render_waveform(&self, f: &mut Frame<Target>, open: &OpenRecording, r: Rect) {
        self.palette
            .push_surface(f, r.x, r.y, r.w, r.h, 4.0, Surface::Card);
        let mid = r.y + r.h / 2.0;
        let half = (r.h / 2.0 - 4.0).max(1.0);
        f.push(RenderCommand::FillRect {
            x: r.x,
            y: mid,
            width: r.w,
            height: 1.0,
            color: self.palette.surface1,
            corner_radii: CornerRadii::ZERO,
        });
        let peaks = &open.peaks;
        let columns = (r.w.floor() as usize).max(1);
        if !peaks.is_empty() && open.info.frames > 0 {
            for c in 0..columns {
                let edge = |c: usize| {
                    c.saturating_mul(peaks.len())
                        .checked_div(columns)
                        .unwrap_or(0)
                };
                let from = edge(c);
                let to = edge(c.saturating_add(1))
                    .max(from.saturating_add(1))
                    .min(peaks.len());
                let (low, high) = peaks
                    .get(from..to)
                    .unwrap_or(&[])
                    .iter()
                    .fold((0.0_f32, 0.0_f32), |(lo, hi), p| {
                        (lo.min(p.low), hi.max(p.high))
                    });
                let top = mid - high * half;
                let bottom = mid - low * half;
                f.push(RenderCommand::FillRect {
                    x: r.x + c as f32,
                    y: top,
                    width: 1.0,
                    height: (bottom - top).max(1.0),
                    color: self.palette.blue,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }
        f.extend(open.kept.render(&self.palette, r.x, r.y, r.w, r.h));
        let frames = open.info.frames;
        for (i, marker) in open.markers.iter().enumerate() {
            let mx = frame_x(r, u64::from(marker.frame), frames);
            let chosen = open.chosen_marker == Some(i);
            f.push(RenderCommand::Line {
                x1: mx,
                y1: r.y,
                x2: mx,
                y2: r.y + r.h,
                color: marker_color(i),
                width: if chosen { 3.0 } else { 2.0 },
            });
            self.text(
                f,
                mx + 4.0,
                r.y + 3.0,
                shown_label(&marker.label),
                10.0,
                self.palette.text,
                90.0,
            );
        }
        let cx = frame_x(r, open.cursor, frames);
        f.push(RenderCommand::Line {
            x1: cx,
            y1: r.y,
            x2: cx,
            y2: r.y + r.h,
            color: self.palette.peach,
            width: 2.0,
        });
        f.hit(Target::Waveform, r);
    }

    fn render_markers(&self, f: &mut Frame<Target>, open: &OpenRecording) {
        let (pane, rows) = self.marker_pane();
        self.heading(
            f,
            pane.x + 16.0,
            pane.y - 22.0,
            format!("MARKERS ({})", open.markers.len()),
            pane.w - 32.0,
        );
        f.hit(Target::MarkerList, pane);
        if open.markers.is_empty() {
            self.text(
                f,
                pane.x + 16.0,
                pane.y + 4.0,
                String::from(
                    "None. M puts one at the cursor; a press on the waveform moves the cursor.",
                ),
                11.0,
                self.palette.subtext0,
                pane.w - 32.0,
            );
        }
        for (shown, (i, marker)) in open
            .markers
            .iter()
            .enumerate()
            .skip(self.marker_scroll)
            .take(rows)
            .enumerate()
        {
            let y = pane.y + shown as f32 * MARKER_ROW_H;
            let row = Rect::new(
                pane.x + 12.0,
                y,
                (pane.w - 24.0).max(0.0),
                MARKER_ROW_H - 2.0,
            );
            let chosen = open.chosen_marker == Some(i);
            if chosen || self.hover == Some(Target::MarkerRow(i)) {
                self.palette.push_surface(
                    f,
                    row.x,
                    row.y,
                    row.w,
                    row.h,
                    4.0,
                    if chosen {
                        Surface::Selected
                    } else {
                        Surface::Card
                    },
                );
            }
            f.push(RenderCommand::FillRect {
                x: row.x + 6.0,
                y: y + 8.0,
                width: 8.0,
                height: 8.0,
                color: marker_color(i),
                corner_radii: CornerRadii::all(4.0),
            });
            self.text(
                f,
                row.x + 22.0,
                y + 5.0,
                clock(open.seconds_at(u64::from(marker.frame))),
                12.0,
                self.palette.subtext1,
                80.0,
            );
            let name = Rect::new(
                row.x + 104.0,
                y + 1.0,
                (row.w - 110.0).max(0.0),
                MARKER_ROW_H - 4.0,
            );
            match (&self.rename, chosen) {
                (Some(input), true) => self.render_field(f, input, name),
                _ => self.text(
                    f,
                    name.x,
                    y + 5.0,
                    shown_label(&marker.label),
                    12.0,
                    self.palette.text,
                    name.w,
                ),
            }
            f.hit(Target::MarkerRow(i), row);
        }
    }

    /// The marker name being written.
    fn render_field(&self, f: &mut Frame<Target>, input: &TextInput, rect: Rect) {
        self.palette
            .push_surface(f, rect.x, rect.y, rect.w, rect.h, 4.0, Surface::Card);
        f.push(RenderCommand::StrokeRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: self.palette.blue,
            line_width: 2.0,
            corner_radii: CornerRadii::all(4.0),
        });
        let mut tree = RenderTree::new();
        textedit::draw(
            &mut tree,
            &textedit::SingleLine {
                text: input.text(),
                cursor: input.cursor(),
                selection_anchor: input.selection_anchor(),
                focused: true,
                x: rect.x + 6.0,
                y: rect.y + 3.0,
                width: (rect.w - 12.0).max(0.0),
                line_height: 16.0,
                font_size: 13.0,
                weight: FontWeightHint::Regular,
                color: self.palette.text,
                selection_bg: self.palette.blue,
                selection_fg: self.palette.crust,
                caret_width: textedit::CARET_WIDTH,
            },
        );
        f.extend(tree.commands);
    }

    fn render_transport(&self, f: &mut Frame<Target>) {
        let y = self.window_height - STATUS_H - TRANSPORT_H;
        let x = SIDEBAR_W;
        let w = (self.window_width - x).max(0.0);
        self.palette
            .push_surface(f, x, y, w, TRANSPORT_H, 0.0, Surface::Card);
        let by = y + 14.0;
        let mut after = x + 16.0;
        match self.state {
            RecordingState::Idle | RecordingState::Stopped => {
                self.buttons_from(
                    f,
                    x + 16.0,
                    by,
                    &[
                        ("\u{25CF} Record", 96.0, Target::Record, true),
                        ("\u{25B6} Play", 80.0, Target::Play, true),
                    ],
                );
                after += 96.0 + 6.0 + 80.0 + 12.0;
            }
            RecordingState::Recording | RecordingState::Paused => {
                let pause = if self.state == RecordingState::Recording {
                    "Pause"
                } else {
                    "Resume"
                };
                self.buttons_from(
                    f,
                    x + 16.0,
                    by,
                    &[
                        (pause, 80.0, Target::Pause, true),
                        ("Stop", 72.0, Target::Stop, true),
                        ("Marker", 80.0, Target::DropMarker, true),
                    ],
                );
                after += 80.0 + 6.0 + 72.0 + 6.0 + 80.0 + 12.0;
                self.text(
                    f,
                    after,
                    by + 6.0,
                    format!(
                        "{}  \u{00B7}  {}",
                        self.timer.format_elapsed(),
                        self.preset.label()
                    ),
                    13.0,
                    self.palette.text,
                    180.0,
                );
                after += 190.0;
            }
        }
        if let Some(reason) = &self.blocked_reason {
            self.text(
                f,
                after,
                by + 7.0,
                reason.clone(),
                11.0,
                self.palette.ink(self.palette.yellow),
                x + w - after - 16.0,
            );
        }
    }

    fn render_status(&self, f: &mut Frame<Target>) {
        let y = self.window_height - STATUS_H;
        self.palette.push_surface(
            f,
            0.0,
            y,
            self.window_width,
            STATUS_H,
            0.0,
            Surface::Strip(appearance::Edge::Top),
        );
        self.text(
            f,
            12.0,
            y + 5.0,
            self.status_line.clone(),
            11.0,
            self.palette.subtext1,
            self.window_width - 24.0,
        );
    }

    // -----------------------------------------------------------------------
    // The pointer
    // -----------------------------------------------------------------------

    /// What is under `(x, y)` in the frame last shown.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if self.last_hits.is_empty() {
            return self.frame().hit_test(x, y);
        }
        self.last_hits
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(target, _)| *target)
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                self.drag = None;
                let Some(target) = self.frame().hit_test(event.x, event.y) else {
                    return EventResult::Ignored;
                };
                // A press anywhere else keeps the name being written.
                if self.rename.is_some() && !matches!(target, Target::MarkerRow(_)) {
                    self.commit_rename();
                }
                let result = self.press(target, event.x);
                self.clamp_scrolls();
                result
            }
            MouseEventKind::Release(MouseButton::Left) => {
                if self.drag.take().is_some() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            MouseEventKind::Move => {
                if self.drag.is_some() {
                    return self.drag_to(event.x);
                }
                let over = self.target_at(event.x, event.y);
                if over == self.hover {
                    return EventResult::Ignored;
                }
                self.hover = over;
                EventResult::Consumed
            }
            MouseEventKind::Leave => {
                self.drag = None;
                if self.hover.take().is_some() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            MouseEventKind::Scroll { dy, .. } => self.wheel_at(event.x, event.y, dy),
            _ => EventResult::Ignored,
        }
    }

    /// A press on `target`, at `x` across the window.
    fn press(&mut self, target: Target, x: f32) -> EventResult {
        match target {
            Target::HelpCard => self.show_help = false,
            Target::Help => self.show_help = true,
            Target::Panel(panel) => {
                if self.panel == panel {
                    return EventResult::Ignored;
                }
                self.panel = panel;
            }
            Target::OpenFile => self.open_picker(PickerFor::Open),
            Target::ChooseFolder => self.open_picker(PickerFor::Folder),
            Target::Refresh => {
                self.rescan();
                self.status_line = String::from("Looked in the folder again");
            }
            Target::LibraryList => {
                if self.panel == Panel::Library {
                    return EventResult::Ignored;
                }
                self.panel = Panel::Library;
            }
            // A press opens a recording: it changes nothing on disk, so it
            // needs no second press to confirm.
            Target::LibraryRow(i) => {
                let Some(path) = self.library.get(i).map(|e| e.path.clone()) else {
                    return EventResult::Ignored;
                };
                self.chosen_entry = Some(path.clone());
                self.open_path(&path);
            }
            Target::Waveform => return self.press_waveform(x),
            Target::MarkerList => {
                if self.panel == Panel::Recording {
                    return EventResult::Ignored;
                }
                self.panel = Panel::Recording;
            }
            Target::MarkerRow(i) => {
                self.panel = Panel::Recording;
                if self.rename.is_some() && self.chosen_marker() == Some(i) {
                    return EventResult::Ignored;
                }
                self.rename = None;
                self.with_open(|o| {
                    o.choose_marker(i);
                });
            }
            Target::AddMarker => self.with_open(|o| {
                o.add_marker();
            }),
            Target::RenameMarker => self.start_rename(),
            Target::RemoveMarker => self.with_open(|o| {
                o.remove_marker();
            }),
            Target::KeepFrom => self.with_open(OpenRecording::keep_from_cursor),
            Target::KeepTo => self.with_open(OpenRecording::keep_to_cursor),
            Target::KeepAll => self.with_open(OpenRecording::keep_all),
            Target::SaveMarkers => {
                self.save_markers();
            }
            Target::SaveKept => self.open_picker(PickerFor::SaveKept),
            Target::Record => {
                self.record_key();
            }
            Target::Pause => {
                self.record_key();
            }
            Target::Stop => {
                self.stop_take();
            }
            Target::DropMarker => {
                let n = self.markers.len().saturating_add(1);
                self.add_marker(format!("Marker {n}"));
            }
            Target::Play => self.play(),
        }
        EventResult::Consumed
    }

    /// A press on the waveform: on a marker it chooses the marker (and a
    /// drag moves it); on a handle of the kept stretch, a drag moves the
    /// handle; anywhere else the cursor goes there and follows a drag.
    fn press_waveform(&mut self, x: f32) -> EventResult {
        let r = self.wave_rect();
        self.panel = Panel::Recording;
        self.press_x = x;
        let Some(open) = self.open.as_mut() else {
            return EventResult::Ignored;
        };
        let frames = open.info.frames;
        let near = |frame: u64| (frame_x(r, frame, frames) - x).abs() <= HANDLE_REACH;
        let drag = if let Some(i) = open.markers.iter().position(|m| near(u64::from(m.frame))) {
            open.choose_marker(i);
            Drag::Marker(i)
        } else if near(open.kept.start_frame) {
            Drag::KeptStart
        } else if near(open.kept.end_frame) {
            Drag::KeptEnd
        } else {
            open.cursor = frame_at(r, x, frames);
            Drag::Cursor
        };
        self.drag = Some(drag);
        EventResult::Consumed
    }

    /// The pointer moved with the button down on the waveform.
    fn drag_to(&mut self, x: f32) -> EventResult {
        let Some(drag) = self.drag else {
            return EventResult::Ignored;
        };
        let r = self.wave_rect();
        let slop = (x - self.press_x).abs() < DRAG_SLOP;
        let Some(open) = self.open.as_mut() else {
            self.drag = None;
            return EventResult::Ignored;
        };
        let frame = frame_at(r, x, open.info.frames);
        match drag {
            Drag::Cursor => open.cursor = frame,
            Drag::KeptStart => {
                open.kept.set_start(frame);
                open.cursor = open.kept.start_frame;
            }
            Drag::KeptEnd => {
                open.kept.set_end(frame);
                open.cursor = open.kept.end_frame;
            }
            Drag::Marker(i) => {
                if slop {
                    return EventResult::Ignored;
                }
                if let Some(at) = open.move_marker(i, frame) {
                    self.drag = Some(Drag::Marker(at));
                }
            }
        }
        EventResult::Consumed
    }
}

impl Default for SoundRecorderApp {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Entry point
// ============================================================================

impl App for SoundRecorderApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        match &self.open {
            Some(open) if open.markers_changed => {
                format!("{} (markers not saved) - Sound Recorder", open.name)
            }
            Some(open) => format!("{} - Sound Recorder", open.name),
            None => String::from("Sound Recorder"),
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        (self.window_width as u32, self.window_height as u32)
    }

    /// A clock only while recording.
    ///
    /// Gated on the state rather than returned unconditionally, because
    /// `tick` and `check_auto_save` both return immediately unless recording
    /// -- so an unconditional interval would wake the machine ten times a
    /// second to do nothing for as long as the window is open.
    ///
    /// 100 ms: the elapsed readout shows tenths.
    fn tick_interval(&self) -> Option<Duration> {
        (self.state == RecordingState::Recording).then(|| Duration::from_millis(100))
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.window_width = width;
        self.window_height = height;
        self.clamp_scrolls();
        let frame = self.frame();
        self.last_hits = frame.hits().to_vec();
        frame.into_tree()
    }
}

fn main() -> ExitCode {
    app::launch("soundrecorder", &mut SoundRecorderApp::from_env())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;

    // -- The take's model ------------------------------------------------------

    #[test]
    fn test_state_idle_transitions() {
        assert!(RecordingState::Idle.can_transition_to(RecordingState::Recording));
        assert!(!RecordingState::Idle.can_transition_to(RecordingState::Paused));
        assert!(!RecordingState::Idle.can_transition_to(RecordingState::Stopped));
        assert!(!RecordingState::Idle.can_transition_to(RecordingState::Idle));
    }

    #[test]
    fn test_state_recording_transitions() {
        assert!(RecordingState::Recording.can_transition_to(RecordingState::Paused));
        assert!(RecordingState::Recording.can_transition_to(RecordingState::Stopped));
        assert!(!RecordingState::Recording.can_transition_to(RecordingState::Idle));
        assert!(!RecordingState::Recording.can_transition_to(RecordingState::Recording));
    }

    #[test]
    fn test_state_paused_transitions() {
        assert!(RecordingState::Paused.can_transition_to(RecordingState::Recording));
        assert!(RecordingState::Paused.can_transition_to(RecordingState::Stopped));
        assert!(!RecordingState::Paused.can_transition_to(RecordingState::Idle));
        assert!(!RecordingState::Paused.can_transition_to(RecordingState::Paused));
    }

    #[test]
    fn test_state_stopped_transitions() {
        assert!(RecordingState::Stopped.can_transition_to(RecordingState::Idle));
        assert!(!RecordingState::Stopped.can_transition_to(RecordingState::Recording));
        assert!(!RecordingState::Stopped.can_transition_to(RecordingState::Paused));
        assert!(!RecordingState::Stopped.can_transition_to(RecordingState::Stopped));
    }

    #[test]
    fn test_state_labels() {
        assert_eq!(RecordingState::Idle.label(), "Idle");
        assert_eq!(RecordingState::Recording.label(), "Recording");
        assert_eq!(RecordingState::Paused.label(), "Paused");
        assert_eq!(RecordingState::Stopped.label(), "Stopped");
    }

    #[test]
    fn test_state_colors_differ() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let idle = RecordingState::Idle.color(&pal);
        let recording = RecordingState::Recording.color(&pal);
        let paused = RecordingState::Paused.color(&pal);
        let stopped = RecordingState::Stopped.color(&pal);
        assert_ne!(idle, recording);
        assert_ne!(recording, paused);
        assert_ne!(paused, stopped);
    }

    #[test]
    fn test_sample_rate_values() {
        assert_eq!(SampleRate::Hz8000.hz(), 8000);
        assert_eq!(SampleRate::Hz22050.hz(), 22050);
        assert_eq!(SampleRate::Hz44100.hz(), 44100);
        assert_eq!(SampleRate::Hz48000.hz(), 48000);
    }

    #[test]
    fn test_sample_rate_bytes_per_second() {
        assert_eq!(SampleRate::Hz48000.bytes_per_second_mono(), 96000);
        assert_eq!(SampleRate::Hz48000.bytes_per_second_stereo(), 192000);
        assert_eq!(SampleRate::Hz44100.bytes_per_second_mono(), 88200);
    }

    #[test]
    fn test_sample_rate_all() {
        let all = SampleRate::all();
        assert_eq!(all.len(), 4);
    }

    #[test]
    fn test_sample_rate_labels_non_empty() {
        for &rate in SampleRate::all() {
            assert!(!rate.label().is_empty());
        }
    }

    #[test]
    fn test_preset_voice() {
        let p = QualityPreset::Voice;
        assert_eq!(p.sample_rate(), SampleRate::Hz8000);
        assert_eq!(p.channels(), 1);
        assert_eq!(p.bits_per_sample(), 16);
    }

    #[test]
    fn test_preset_music() {
        let p = QualityPreset::Music;
        assert_eq!(p.sample_rate(), SampleRate::Hz44100);
        assert_eq!(p.channels(), 2);
    }

    #[test]
    fn test_preset_lossless() {
        let p = QualityPreset::Lossless;
        assert_eq!(p.sample_rate(), SampleRate::Hz48000);
        assert_eq!(p.channels(), 2);
    }

    #[test]
    fn test_preset_bytes_per_second() {
        // Voice: 8000 * 1 * 2 = 16000
        assert_eq!(QualityPreset::Voice.bytes_per_second(), 16000);
        // Music: 44100 * 2 * 2 = 176400
        assert_eq!(QualityPreset::Music.bytes_per_second(), 176400);
        // Lossless: 48000 * 2 * 2 = 192000
        assert_eq!(QualityPreset::Lossless.bytes_per_second(), 192000);
    }

    #[test]
    fn test_preset_all() {
        assert_eq!(QualityPreset::all().len(), 3);
    }

    #[test]
    fn test_mock_devices_not_empty() {
        let devices = AudioInputDevice::mock_devices();
        assert!(!devices.is_empty());
    }

    #[test]
    fn test_mock_devices_have_default() {
        let devices = AudioInputDevice::mock_devices();
        assert!(devices.iter().any(|d| d.is_default));
    }

    #[test]
    fn test_mock_devices_unique_ids() {
        let devices = AudioInputDevice::mock_devices();
        let ids: Vec<u32> = devices.iter().map(|d| d.id).collect();
        for (i, id) in ids.iter().enumerate() {
            assert!(!ids[i + 1..].contains(id), "duplicate device id {id}");
        }
    }

    #[test]
    fn test_wav_new_empty() {
        let wav = WavFile::new(44100, 2, 16);
        assert_eq!(wav.frame_count(), 0);
        assert_eq!(wav.duration_secs(), 0.0);
        assert_eq!(wav.data_size(), 0);
    }

    #[test]
    fn test_wav_push_samples() {
        let mut wav = WavFile::new(44100, 1, 16);
        wav.push_samples(&[100, 200, 300]);
        assert_eq!(wav.samples.len(), 3);
        assert_eq!(wav.frame_count(), 3);
    }

    #[test]
    fn test_wav_stereo_frame_count() {
        let mut wav = WavFile::new(44100, 2, 16);
        wav.push_samples(&[100, 200, 300, 400]); // 2 frames of stereo
        assert_eq!(wav.frame_count(), 2);
    }

    #[test]
    fn test_wav_duration() {
        let mut wav = WavFile::new(44100, 1, 16);
        let samples: Vec<i16> = vec![0; 44100]; // 1 second
        wav.push_samples(&samples);
        assert!((wav.duration_secs() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_wav_block_align() {
        assert_eq!(WavFile::new(44100, 1, 16).block_align(), 2);
        assert_eq!(WavFile::new(44100, 2, 16).block_align(), 4);
    }

    #[test]
    fn test_wav_byte_rate() {
        let wav = WavFile::new(44100, 2, 16);
        // 44100 * 4 = 176400
        assert_eq!(wav.byte_rate(), 176400);
    }

    #[test]
    fn test_wav_from_preset() {
        let wav = WavFile::from_preset(QualityPreset::Voice);
        assert_eq!(wav.sample_rate, 8000);
        assert_eq!(wav.channels, 1);
        assert_eq!(wav.bits_per_sample, 16);
    }

    #[test]
    fn test_wav_zero_channels_frame_count() {
        let wav = WavFile::new(44100, 0, 16);
        assert_eq!(wav.frame_count(), 0);
    }

    #[test]
    fn test_wav_zero_sample_rate_duration() {
        let wav = WavFile::new(0, 1, 16);
        assert_eq!(wav.duration_secs(), 0.0);
    }

    #[test]
    fn test_waveform_push_and_len() {
        let mut wf = WaveformDisplay::new(0.0, 0.0, 100.0, 50.0);
        assert!(wf.is_empty());
        wf.push_amplitude(0.5);
        assert_eq!(wf.len(), 1);
    }

    #[test]
    fn test_waveform_clamp() {
        let mut wf = WaveformDisplay::new(0.0, 0.0, 10.0, 10.0);
        wf.push_amplitude(2.0); // should clamp to 1.0
        wf.push_amplitude(-1.0); // should clamp to 0.0
        assert_eq!(wf.len(), 2);
    }

    #[test]
    fn test_waveform_scrolling() {
        let mut wf = WaveformDisplay::new(0.0, 0.0, 3.0, 10.0);
        // max_columns = 3
        wf.push_amplitude(0.1);
        wf.push_amplitude(0.2);
        wf.push_amplitude(0.3);
        wf.push_amplitude(0.4); // should push out 0.1
        assert_eq!(wf.len(), 3);
    }

    #[test]
    fn test_waveform_clear() {
        let mut wf = WaveformDisplay::new(0.0, 0.0, 100.0, 50.0);
        wf.push_amplitude(0.5);
        wf.clear();
        assert!(wf.is_empty());
    }

    #[test]
    fn test_waveform_amplitude_from_samples() {
        assert_eq!(WaveformDisplay::amplitude_from_samples(&[]), 0.0);
        let amp = WaveformDisplay::amplitude_from_samples(&[16384, -16384]);
        assert!((amp - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_waveform_render_nonempty() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut wf = WaveformDisplay::new(0.0, 0.0, 100.0, 50.0);
        wf.push_amplitude(0.5);
        let cmds = wf.render(&pal);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_vu_meter_initial_zero() {
        let vu = VuMeter::new(0.0, 0.0, 100.0, 20.0);
        assert_eq!(vu.current_level, 0.0);
        assert_eq!(vu.peak_level, 0.0);
    }

    #[test]
    fn test_vu_meter_instant_attack() {
        let mut vu = VuMeter::new(0.0, 0.0, 100.0, 20.0);
        vu.update(0.8);
        assert_eq!(vu.current_level, 0.8);
    }

    #[test]
    fn test_vu_meter_decay() {
        let mut vu = VuMeter::new(0.0, 0.0, 100.0, 20.0);
        vu.update(1.0);
        vu.update(0.0);
        assert!(vu.current_level < 1.0);
        assert!(vu.current_level > 0.0);
    }

    #[test]
    fn test_vu_meter_peak_hold() {
        let mut vu = VuMeter::new(0.0, 0.0, 100.0, 20.0);
        vu.update(0.9);
        vu.update(0.1);
        // Peak should still be 0.9 (hold period not expired)
        assert_eq!(vu.peak_level, 0.9);
    }

    #[test]
    fn test_vu_meter_reset() {
        let mut vu = VuMeter::new(0.0, 0.0, 100.0, 20.0);
        vu.update(0.5);
        vu.reset();
        assert_eq!(vu.current_level, 0.0);
        assert_eq!(vu.peak_level, 0.0);
    }

    #[test]
    fn test_vu_meter_render_produces_commands() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let vu = VuMeter::new(0.0, 0.0, 100.0, 20.0);
        let cmds = vu.render(&pal);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_vu_meter_level_color_ranges() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        assert_eq!(VuMeter::level_color(0.3, &pal), pal.green);
        assert_eq!(VuMeter::level_color(0.7, &pal), pal.yellow);
        assert_eq!(VuMeter::level_color(0.95, &pal), pal.red);
    }

    #[test]
    fn test_timer_initial_zero() {
        let t = RecordingTimer::new(Some(1_000_000), 192000);
        assert_eq!(t.elapsed_secs(), 0.0);
    }

    #[test]
    fn test_timer_tick() {
        let mut t = RecordingTimer::new(Some(1_000_000), 192000);
        t.tick(1500);
        assert!((t.elapsed_secs() - 1.5).abs() < 0.001);
    }

    #[test]
    fn test_timer_format_elapsed() {
        let mut t = RecordingTimer::new(Some(0), 0);
        t.tick(3_661_000); // 1h 1m 1s
        assert_eq!(t.format_elapsed(), "01:01:01");
    }

    #[test]
    fn test_timer_remaining() {
        let t = RecordingTimer::new(Some(384_000), 192000);
        assert!((t.remaining_secs().expect("space was set") - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_timer_remaining_zero_rate() {
        let t = RecordingTimer::new(Some(1000), 0);
        assert!(t.remaining_secs().expect("space was set").is_infinite());
    }

    #[test]
    fn test_timer_format_remaining_overflow() {
        let t = RecordingTimer::new(Some(u64::MAX), 1);
        assert_eq!(t.format_remaining(), "99:59:59+");
    }

    #[test]
    fn test_timer_reset() {
        let mut t = RecordingTimer::new(Some(1000), 100);
        t.tick(5000);
        t.reset();
        assert_eq!(t.elapsed_secs(), 0.0);
    }

    #[test]
    fn test_timer_render_produces_commands() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let t = RecordingTimer::new(Some(1000), 100);
        let cmds = t.render(&pal, 0.0, 0.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_markers_empty() {
        let m = MarkerList::new();
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn test_markers_add_and_get() {
        let mut m = MarkerList::new();
        let id = m.add(1000, "Intro".into());
        assert_eq!(m.len(), 1);
        let marker = m.get(id).expect("marker not found");
        assert_eq!(marker.frame_position, 1000);
        assert_eq!(marker.label, "Intro");
    }

    #[test]
    fn test_markers_remove() {
        let mut m = MarkerList::new();
        let id = m.add(0, "X".into());
        assert!(m.remove(id));
        assert!(m.is_empty());
    }

    #[test]
    fn test_markers_remove_nonexistent() {
        let mut m = MarkerList::new();
        assert!(!m.remove(999));
    }

    #[test]
    fn test_markers_sorted() {
        let mut m = MarkerList::new();
        m.add(3000, "C".into());
        m.add(1000, "A".into());
        m.add(2000, "B".into());
        let sorted = m.sorted();
        assert_eq!(sorted[0].frame_position, 1000);
        assert_eq!(sorted[1].frame_position, 2000);
        assert_eq!(sorted[2].frame_position, 3000);
    }

    #[test]
    fn test_markers_clear() {
        let mut m = MarkerList::new();
        m.add(0, "A".into());
        m.add(0, "B".into());
        m.clear();
        assert!(m.is_empty());
    }

    #[test]
    fn test_markers_render_empty() {
        let m = MarkerList::new();
        let cmds = m.render(1000, 0.0, 0.0, 100.0, 50.0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_markers_render_with_entries() {
        let mut m = MarkerList::new();
        m.add(500, "Mid".into());
        let cmds = m.render(1000, 0.0, 0.0, 100.0, 50.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_trim_full() {
        let t = TrimRegion::full(1000);
        assert!(t.is_full());
        assert_eq!(t.length_frames(), 1000);
    }

    #[test]
    fn test_trim_set_start() {
        let mut t = TrimRegion::full(1000);
        t.set_start(200);
        assert_eq!(t.start_frame, 200);
        assert_eq!(t.length_frames(), 800);
    }

    #[test]
    fn test_trim_set_end() {
        let mut t = TrimRegion::full(1000);
        t.set_end(800);
        assert_eq!(t.end_frame, 800);
        assert_eq!(t.length_frames(), 800);
    }

    #[test]
    fn test_trim_start_clamp() {
        let mut t = TrimRegion::full(1000);
        t.set_end(500);
        t.set_start(600); // should clamp to 499
        assert!(t.start_frame < t.end_frame);
    }

    #[test]
    fn test_trim_end_clamp() {
        let mut t = TrimRegion::full(1000);
        t.set_start(500);
        t.set_end(200); // should clamp to at least start+1 = 501
        assert!(t.end_frame > t.start_frame);
    }

    #[test]
    fn test_trim_duration() {
        let mut t = TrimRegion::full(48000);
        t.set_start(0);
        t.set_end(24000);
        assert!((t.duration_secs(48000) - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_trim_apply() {
        let samples: Vec<i16> = (0..10).collect();
        let mut t = TrimRegion::full(10);
        t.set_start(2);
        t.set_end(5);
        let trimmed = t.apply(&samples, 1);
        assert_eq!(trimmed, vec![2, 3, 4]);
    }

    #[test]
    fn test_trim_apply_stereo() {
        // 5 frames of stereo = 10 samples
        let samples: Vec<i16> = (0..10).collect();
        let mut t = TrimRegion::full(5);
        t.set_start(1);
        t.set_end(3);
        let trimmed = t.apply(&samples, 2);
        assert_eq!(trimmed, vec![2, 3, 4, 5]);
    }

    #[test]
    fn test_trim_render_full_no_dim() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let t = TrimRegion::full(1000);
        let cmds = t.render(&pal, 0.0, 0.0, 100.0, 50.0);
        // Should have 2 handle rects but no dim rects
        assert_eq!(cmds.len(), 2);
    }

    #[test]
    fn test_trim_render_partial_has_dim() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut t = TrimRegion::full(1000);
        t.set_start(200);
        t.set_end(800);
        let cmds = t.render(&pal, 0.0, 0.0, 100.0, 50.0);
        // 2 dim regions + 2 handles = 4
        assert_eq!(cmds.len(), 4);
    }

    #[test]
    fn test_noise_gate_creation() {
        let ng = NoiseGate::new(0.05);
        assert!((ng.threshold - 0.05).abs() < 0.001);
        assert!(ng.enabled);
        assert!(!ng.is_open);
    }

    #[test]
    fn test_noise_gate_threshold_clamp() {
        let mut ng = NoiseGate::new(0.5);
        ng.set_threshold(2.0);
        assert_eq!(ng.threshold, 1.0);
        ng.set_threshold(-1.0);
        assert_eq!(ng.threshold, 0.0);
    }

    #[test]
    fn test_noise_gate_silence_zeroed() {
        let mut ng = NoiseGate::new(0.5);
        let mut samples = [100i16, 50, 30, 10];
        ng.process(&mut samples);
        // All below threshold (0.5 * 32767 ~ 16383): should be zeroed
        assert!(samples.iter().all(|&s| s == 0));
    }

    #[test]
    fn test_noise_gate_loud_passes() {
        let mut ng = NoiseGate::new(0.01);
        let mut samples = [20000i16, -20000, 15000];
        let passed = ng.process(&mut samples);
        assert!(passed);
        assert!(ng.is_open);
    }

    #[test]
    fn test_noise_gate_disabled() {
        let mut ng = NoiseGate::new(0.5);
        ng.enabled = false;
        let mut samples = [10i16, 20, 30];
        let passed = ng.process(&mut samples);
        assert!(passed);
        assert_eq!(samples, [10, 20, 30]);
    }

    #[test]
    fn test_noise_gate_reset() {
        let mut ng = NoiseGate::new(0.01);
        let mut samples = [20000i16];
        ng.process(&mut samples);
        assert!(ng.is_open);
        ng.reset();
        assert!(!ng.is_open);
    }

    #[test]
    fn test_noise_gate_render_produces_commands() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let ng = NoiseGate::new(0.1);
        let cmds = ng.render(&pal, 0.0, 0.0, 100.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_playback_initial_state() {
        let pb = PlaybackController::new();
        assert_eq!(pb.state, PlaybackState::Stopped);
        assert_eq!(pb.position_frames, 0);
    }

    #[test]
    fn test_playback_load_and_play() {
        let mut pb = PlaybackController::new();
        pb.load(48000, 48000);
        pb.play();
        assert_eq!(pb.state, PlaybackState::Playing);
    }

    #[test]
    fn test_playback_pause() {
        let mut pb = PlaybackController::new();
        pb.load(48000, 48000);
        pb.play();
        pb.pause();
        assert_eq!(pb.state, PlaybackState::Paused);
    }

    #[test]
    fn test_playback_stop_resets_position() {
        let mut pb = PlaybackController::new();
        pb.load(48000, 48000);
        pb.play();
        pb.advance(1000);
        pb.stop();
        assert_eq!(pb.position_frames, 0);
        assert_eq!(pb.state, PlaybackState::Stopped);
    }

    #[test]
    fn test_playback_seek() {
        let mut pb = PlaybackController::new();
        pb.load(48000, 48000);
        pb.seek(24000);
        assert_eq!(pb.position_frames, 24000);
    }

    #[test]
    fn test_playback_seek_clamp() {
        let mut pb = PlaybackController::new();
        pb.load(48000, 48000);
        pb.seek(100000);
        assert_eq!(pb.position_frames, 48000);
    }

    #[test]
    fn test_playback_seek_relative() {
        let mut pb = PlaybackController::new();
        pb.load(96000, 48000); // 2 seconds
        pb.seek(48000); // 1 second in
        pb.seek_relative(0.5); // forward 0.5s
        assert_eq!(pb.position_frames, 72000);
    }

    #[test]
    fn test_playback_seek_relative_negative() {
        let mut pb = PlaybackController::new();
        pb.load(96000, 48000);
        pb.seek(48000);
        pb.seek_relative(-2.0); // should clamp to 0
        assert_eq!(pb.position_frames, 0);
    }

    #[test]
    fn test_playback_advance_end() {
        let mut pb = PlaybackController::new();
        pb.load(100, 48000);
        pb.play();
        let finished = pb.advance(150);
        assert!(finished);
        assert_eq!(pb.state, PlaybackState::Stopped);
    }

    #[test]
    fn test_playback_progress() {
        let mut pb = PlaybackController::new();
        pb.load(1000, 48000);
        pb.seek(500);
        assert!((pb.progress() - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_playback_format_position() {
        let mut pb = PlaybackController::new();
        pb.load(48000 * 65, 48000); // 65 seconds
        pb.seek(48000 * 65);
        assert_eq!(pb.format_position(), "01:05");
    }

    #[test]
    fn test_playback_render_produces_commands() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let pb = PlaybackController::new();
        let cmds = pb.render(&pal, 0.0, 0.0, 100.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_autosave_disabled() {
        let mut auto = AutoSave::new(0);
        assert!(!auto.enabled);
        assert!(!auto.tick(10000));
    }

    #[test]
    fn test_autosave_triggers() {
        let mut auto = AutoSave::new(5); // 5 seconds
        assert!(!auto.tick(3000));
        assert!(auto.tick(3000)); // 6000 ms >= 5000 ms
        assert_eq!(auto.save_count, 1);
    }

    #[test]
    fn test_autosave_multiple_triggers() {
        let mut auto = AutoSave::new(1); // 1 second
        auto.tick(1500);
        auto.tick(1500);
        assert_eq!(auto.save_count, 2);
    }

    #[test]
    fn test_autosave_reset() {
        let mut auto = AutoSave::new(5);
        auto.tick(4000);
        auto.reset_timer();
        assert!(!auto.tick(2000)); // only 2s since reset, not 5
    }

    #[test]
    fn test_app_creation() {
        let app = SoundRecorderApp::with_mock_input();
        assert_eq!(app.state, RecordingState::Idle);
        assert_eq!(app.preset, QualityPreset::Music);
    }

    #[test]
    fn test_app_start_recording() {
        let mut app = SoundRecorderApp::with_mock_input();
        assert!(app.transition_to(RecordingState::Recording));
        assert_eq!(app.state, RecordingState::Recording);
    }

    #[test]
    fn test_app_full_lifecycle() {
        let mut app = SoundRecorderApp::with_mock_input();
        assert!(app.transition_to(RecordingState::Recording));
        assert!(app.transition_to(RecordingState::Paused));
        assert!(app.transition_to(RecordingState::Recording)); // resume
        assert!(app.transition_to(RecordingState::Stopped));
        assert!(app.transition_to(RecordingState::Idle));
    }

    #[test]
    fn test_app_invalid_transition() {
        let mut app = SoundRecorderApp::with_mock_input();
        assert!(!app.transition_to(RecordingState::Stopped));
        assert_eq!(app.state, RecordingState::Idle);
    }

    #[test]
    fn test_app_process_samples() {
        let mut app = SoundRecorderApp::with_mock_input();
        app.transition_to(RecordingState::Recording);
        app.process_samples(&[5000, -5000, 3000, -3000]);
        assert!(!app.wav.samples.is_empty());
    }

    #[test]
    fn test_app_process_samples_ignored_when_idle() {
        let mut app = SoundRecorderApp::with_mock_input();
        app.process_samples(&[5000, -5000]);
        assert_eq!(app.wav.samples.len(), 0);
    }

    #[test]
    fn test_app_set_preset() {
        let mut app = SoundRecorderApp::with_mock_input();
        app.set_preset(QualityPreset::Voice);
        assert_eq!(app.preset, QualityPreset::Voice);
        assert_eq!(app.sample_rate, SampleRate::Hz8000);
    }

    #[test]
    fn test_app_select_device() {
        let mut app = SoundRecorderApp::with_mock_input();
        assert!(app.select_device(1));
        assert_eq!(app.selected_device, 1);
        assert!(!app.select_device(999));
    }

    #[test]
    fn test_app_add_marker_during_recording() {
        let mut app = SoundRecorderApp::with_mock_input();
        app.transition_to(RecordingState::Recording);
        let id = app.add_marker("Test".into());
        assert!(id.is_some());
        assert_eq!(app.markers.len(), 1);
    }

    #[test]
    fn test_app_add_marker_idle_fails() {
        let mut app = SoundRecorderApp::with_mock_input();
        assert!(app.add_marker("Test".into()).is_none());
    }

    #[test]
    fn test_app_tick() {
        let mut app = SoundRecorderApp::with_mock_input();
        app.transition_to(RecordingState::Recording);
        app.tick(1000);
        assert!((app.timer.elapsed_secs() - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_app_tick_idle_no_effect() {
        let mut app = SoundRecorderApp::with_mock_input();
        app.tick(5000);
        assert_eq!(app.timer.elapsed_secs(), 0.0);
    }

    #[test]
    fn test_app_current_device() {
        let app = SoundRecorderApp::with_mock_input();
        let device = app.current_device().expect("should have default device");
        assert!(device.is_default);
    }

    #[test]
    fn test_app_auto_save_during_recording() {
        let mut app = SoundRecorderApp::with_mock_input();
        app.auto_save.set_interval_secs(1);
        app.transition_to(RecordingState::Recording);
        assert!(!app.check_auto_save(500));
        assert!(app.check_auto_save(600));
    }

    #[test]
    fn test_app_auto_save_idle_no_trigger() {
        let mut app = SoundRecorderApp::with_mock_input();
        app.auto_save.set_interval_secs(1);
        assert!(!app.check_auto_save(5000));
    }

    // -- The window, driven as a user drives it --------------------------------

    use guitk::probe::{self, Probe};

    impl Probe for SoundRecorderApp {
        type Target = Target;
        type Outcome = EventResult;
        const SIZE: (f32, f32) = (980.0, 640.0);

        fn draw(&self, _size: (f32, f32)) -> Frame<Target> {
            self.frame()
        }

        fn click_at(
            &mut self,
            x: f32,
            y: f32,
            button: MouseButton,
            _size: (f32, f32),
        ) -> EventResult {
            self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }))
        }

        fn key_at(&mut self, key: &KeyEvent, _size: (f32, f32)) -> EventResult {
            self.handle_event(&Event::Key(key.clone()))
        }

        fn scroll_at(&mut self, x: f32, y: f32, dy: f32, _size: (f32, f32)) -> Option<EventResult> {
            Some(self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            })))
        }
    }

    fn mouse(app: &mut SoundRecorderApp, x: f32, y: f32, kind: MouseEventKind) -> EventResult {
        app.handle_event(&Event::Mouse(MouseEvent { x, y, kind }))
    }

    fn texts(app: &SoundRecorderApp) -> Vec<String> {
        app.frame()
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn key(k: Key) -> Event {
        Event::Key(probe::press(k))
    }

    /// A directory for one test, removed when it ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("soundrecorder-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        /// A 16-bit WAV of a tone, `seconds` long, with `marks` as markers.
        fn wav(
            &self,
            name: &str,
            rate: u32,
            channels: u16,
            seconds: f32,
            marks: &[(u32, &[u8])],
        ) -> PathBuf {
            let frames = (rate as f32 * seconds) as usize;
            let mut samples = Vec::with_capacity(frames * usize::from(channels));
            for i in 0..frames {
                let v = (8_000.0 * (i as f32 * 0.05).sin()) as i16;
                for _ in 0..channels {
                    samples.push(v);
                }
            }
            let bytes = wavpcm::encode_pcm16(rate, channels, &samples).unwrap();
            let cues: Vec<wavpcm::Cue> = marks
                .iter()
                .map(|(frame, label)| wavpcm::Cue {
                    frame: *frame,
                    label: label.to_vec(),
                })
                .collect();
            let bytes = wavpcm::with_cues(&bytes, &cues).unwrap();
            let path = self.0.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            // Best effort: a leftover temporary directory is harmless.
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A recorder listing a folder of two recordings, the first open, with a
    /// marker at a quarter of a second.
    fn fixture(tag: &str) -> (Scratch, SoundRecorderApp) {
        let dir = Scratch::new(tag);
        let a = dir.wav("a.wav", 8_000, 1, 2.0, &[(2_000, b"intro")]);
        dir.wav("b.wav", 8_000, 2, 1.0, &[]);
        let mut app = SoundRecorderApp::new();
        app.recordings_dir = Some(dir.0.clone());
        app.rescan();
        assert!(app.open_path(&a));
        (dir, app)
    }

    /// The take model's clock and the free space it cannot measure.
    ///
    /// The transport used to show `-01:26:48` beside the elapsed clock, which
    /// is 1 GB divided by the bitrate -- a gigabyte nothing had looked up.
    #[test]
    fn unmeasured_free_space_reads_as_unknown() {
        let t = RecordingTimer::new(None, 192_000);
        assert_eq!(t.remaining_secs(), None);
        assert_eq!(t.format_remaining(), "--:--:--");
        let app = SoundRecorderApp::new();
        assert_eq!(app.timer.remaining_secs(), None);
        let drawn = t.render(
            &Palette::from_settings(&appearance::AppearanceSettings::default()),
            0.0,
            0.0,
        );
        assert!(drawn.iter().any(|c| matches!(
            c,
            RenderCommand::Text { text, .. } if text == "--:--:--"
        )));
    }

    /// **Every key the card advertises is answered by this window**, with a
    /// recording open and one in each panel.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let answered = [Panel::Library, Panel::Recording].into_iter().any(|panel| {
                    let (_dir, mut app) = fixture("keys");
                    app.panel = panel;
                    app.chosen_entry = app.library.first().map(|e| e.path.clone());
                    if let Some(open) = app.open.as_mut() {
                        open.choose_marker(0);
                        open.cursor = 4_000;
                    }
                    app.handle_event(&Event::Key(stroke.clone())) == EventResult::Consumed
                });
                assert!(
                    answered,
                    "the card advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// **The card reaches the window, and nothing records behind it.**
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let mut app = SoundRecorderApp::with_mock_input();
        assert!(!texts(&app).iter().any(|t| t.contains("F1 closes this")));
        app.handle_event(&key(Key::F1));
        let missing = guitk::shortcut::missing_rows(&texts(&app), SHORTCUTS);
        assert!(missing.is_empty(), "{missing:?}");
        app.handle_event(&key(Key::Space));
        assert_eq!(
            app.state,
            RecordingState::Idle,
            "Space recorded through the card"
        );
        app.handle_event(&key(Key::F1));
        app.handle_event(&key(Key::Space));
        assert_eq!(
            app.state,
            RecordingState::Recording,
            "control: Space records with the card down"
        );
    }

    /// A take cannot be started with nothing to record from, and no clock
    /// runs: the defect behind all of this, a clock that climbed and an
    /// auto-save that counted while nothing was captured.
    #[test]
    fn a_take_cannot_be_started_without_an_input_and_no_clock_runs() {
        let mut app = SoundRecorderApp::new();
        assert!(!app.transition_to(RecordingState::Recording));
        assert_eq!(app.handle_event(&key(Key::Space)), EventResult::Consumed);
        assert_eq!(app.state, RecordingState::Idle);
        for _ in 0..60 {
            app.tick(1000);
            assert!(!app.check_auto_save(1000));
        }
        assert_eq!(app.timer.elapsed_ms, 0);
        assert_eq!(app.auto_save.save_count, 0);
        let why = app
            .blocked_reason
            .clone()
            .expect("refused and said nothing");
        assert_eq!(why, CANNOT_RECORD);
        assert!(
            texts(&app).contains(&why),
            "the refusal never reached the screen"
        );
    }

    /// And the window says so before anything is pressed, distinguishing a
    /// program that cannot from a machine without a microphone.
    #[test]
    fn the_window_says_it_cannot_record_before_record_is_pressed() {
        let drawn = texts(&SoundRecorderApp::new());
        for line in NO_AUDIO_LINES {
            assert!(
                drawn.iter().any(|t| t == line),
                "the window never said {line:?}"
            );
        }
        assert!(
            NO_AUDIO_LINES
                .iter()
                .any(|l| l.contains("Not a missing microphone"))
        );
        assert!(
            !texts(&SoundRecorderApp::with_mock_input())
                .iter()
                .any(|t| t == NO_AUDIO_LINES[0]),
            "with an input the notice goes"
        );
    }

    /// Play says why it cannot, by key and by press: a button that does
    /// nothing visible reads as broken.
    #[test]
    fn play_says_why_it_cannot() {
        let mut app = SoundRecorderApp::new();
        assert_eq!(app.handle_event(&key(Key::P)), EventResult::Consumed);
        assert_eq!(app.blocked_reason.as_deref(), Some(CANNOT_PLAY));
        let mut app = SoundRecorderApp::new();
        probe::click(&mut app, Target::Play);
        assert_eq!(app.blocked_reason.as_deref(), Some(CANNOT_PLAY));
        assert!(texts(&app).iter().any(|t| t == CANNOT_PLAY));
    }

    /// Space is the take's transport: start, pause, resume.
    #[test]
    fn space_starts_pauses_and_resumes() {
        let mut app = SoundRecorderApp::with_mock_input();
        for want in [
            RecordingState::Recording,
            RecordingState::Paused,
            RecordingState::Recording,
        ] {
            assert_eq!(app.handle_event(&key(Key::Space)), EventResult::Consumed);
            assert_eq!(app.state, want);
        }
    }

    /// Stop ends the take and saves it -- samples as captured, markers as the
    /// file's own -- as a new file in the recordings folder, which it lists
    /// and opens; a second take in the same second gets a name of its own.
    #[test]
    fn stop_saves_the_take_and_opens_it() {
        let dir = Scratch::new("take");
        let mut app = SoundRecorderApp::with_mock_input();
        let folder = dir.0.join("Recordings");
        app.recordings_dir = Some(folder.clone());
        // Somebody's files already hold the names a take made in the next
        // few seconds would take: the take goes beside them, not over them.
        std::fs::create_dir_all(&folder).unwrap();
        let now = SystemTime::now();
        let theirs: Vec<PathBuf> = (0..3)
            .map(|s| folder.join(format!("{}.wav", take_name(now + Duration::from_secs(s)))))
            .collect();
        for path in &theirs {
            std::fs::write(path, b"somebody's").unwrap();
        }
        app.handle_event(&key(Key::Space));
        // Loud enough to open the noise gate, which zeroes what is quieter.
        let samples: Vec<i16> = (0..4_410).map(|i| 1_000 + (i % 300) as i16).collect();
        app.process_samples(&samples);
        app.handle_event(&key(Key::M));
        app.process_samples(&samples);
        assert_eq!(app.handle_event(&key(Key::S)), EventResult::Consumed);
        assert_eq!(app.state, RecordingState::Idle);
        let open = app.open.as_ref().expect("the take was not opened");
        assert!(open.name.starts_with("Recording "), "{}", open.name);
        assert!(open.name.ends_with(" (2).wav"), "{}", open.name);
        for path in &theirs {
            assert_eq!(
                std::fs::read(path).unwrap(),
                b"somebody's",
                "a take replaced {}",
                path.display()
            );
        }
        let bytes = std::fs::read(&open.path).unwrap();
        let info = wavpcm::parse_header(&bytes).unwrap();
        assert_eq!((info.sample_rate, info.channels), (44_100, 2));
        let stored: Vec<i16> = bytes[info.data_offset..info.data_offset + info.data_len]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        let mut both = samples.clone();
        both.extend_from_slice(&samples);
        assert_eq!(stored, both, "the take was not stored as captured");
        let cues = wavpcm::cues(&bytes).unwrap();
        assert_eq!(
            cues,
            vec![wavpcm::Cue {
                frame: 2_205,
                label: b"Marker 1".to_vec()
            }]
        );
        assert_eq!(app.library.len(), 4, "the take and the three already there");
        let first = open.path.clone();
        // Another take, the same second: never over the first.
        app.handle_event(&key(Key::Space));
        app.process_samples(&samples);
        app.handle_event(&key(Key::S));
        let second = app.open.as_ref().unwrap().path.clone();
        assert_ne!(second, first);
        assert_eq!(
            std::fs::read(&first).unwrap(),
            bytes,
            "the first take was written over"
        );
        assert_eq!(app.library.len(), 5);
    }

    /// With nowhere to save, Stop keeps the take and says so, and a second
    /// Stop can try again.
    #[test]
    fn a_take_that_cannot_be_saved_is_kept() {
        let mut app = SoundRecorderApp::with_mock_input();
        app.handle_event(&key(Key::Space));
        app.process_samples(&[100; 64]);
        app.handle_event(&key(Key::S));
        assert_eq!(app.state, RecordingState::Paused);
        assert_eq!(app.wav.samples.len(), 64);
        assert!(app.status_line.contains("not saved"), "{}", app.status_line);
    }

    /// The clock is asked for only while recording.
    #[test]
    fn the_recorder_asks_for_a_clock_only_while_recording() {
        let dir = Scratch::new("clock");
        let mut app = SoundRecorderApp::with_mock_input();
        app.recordings_dir = Some(dir.0.clone());
        assert_eq!(app.tick_interval(), None);
        app.handle_event(&key(Key::Space));
        assert_eq!(app.tick_interval(), Some(Duration::from_millis(100)));
        app.handle_event(&key(Key::S));
        assert_eq!(app.tick_interval(), None);
    }

    /// Elapsed time follows the milliseconds delivered, not the tick count.
    #[test]
    fn elapsed_time_follows_milliseconds_and_not_tick_count() {
        let mut app = SoundRecorderApp::with_mock_input();
        app.handle_event(&key(Key::Space));
        for _ in 0..3 {
            app.handle_event(&Event::Tick { elapsed_ms: 100 });
        }
        let mut other = SoundRecorderApp::with_mock_input();
        other.handle_event(&key(Key::Space));
        other.handle_event(&Event::Tick { elapsed_ms: 300 });
        assert_eq!(app.timer.elapsed_secs(), other.timer.elapsed_secs());
    }

    /// A paused recorder does not accumulate time.
    #[test]
    fn a_paused_recorder_does_not_count_time() {
        let mut app = SoundRecorderApp::with_mock_input();
        app.handle_event(&key(Key::Space));
        app.handle_event(&Event::Tick { elapsed_ms: 500 });
        let while_recording = app.timer.elapsed_secs();
        app.handle_event(&key(Key::Space));
        app.handle_event(&Event::Tick { elapsed_ms: 500 });
        assert_eq!(app.timer.elapsed_secs(), while_recording);
    }

    /// The take's WAV is its samples exactly and its markers.
    #[test]
    fn the_take_is_stored_as_it_was_captured() {
        let mut wav = WavFile::new(8_000, 1, 16);
        wav.push_samples(&[i16::MIN, -1, 0, 1, i16::MAX]);
        let mut marks = MarkerList::new();
        marks.add(3, String::from("here"));
        let bytes = wav.to_wav(&marks).unwrap();
        let info = wavpcm::parse_header(&bytes).unwrap();
        assert_eq!(info.frames, 5);
        assert_eq!(
            &bytes[info.data_offset..info.data_offset + 10],
            &[0x00, 0x80, 0xFF, 0xFF, 0, 0, 1, 0, 0xFF, 0x7F]
        );
        assert_eq!(wavpcm::cues(&bytes).unwrap()[0].label, b"here");
    }

    /// The folder is listed with each file's real length and format; what is
    /// not a WAV this reads says why; what is not a WAV at all is left out;
    /// and a file past the megabyte read for its header has its whole length.
    #[test]
    fn the_folder_lists_its_recordings_as_they_are() {
        let dir = Scratch::new("list");
        dir.wav("b.WAV", 8_000, 2, 1.0, &[]);
        dir.wav("a.wav", 8_000, 1, 2.0, &[]);
        dir.wav("long.wav", 8_000, 1, 70.0, &[]);
        std::fs::write(dir.0.join("broken.wav"), b"not a riff at all").unwrap();
        std::fs::write(dir.0.join("notes.txt"), b"hello").unwrap();
        let mut app = SoundRecorderApp::new();
        app.recordings_dir = Some(dir.0.clone());
        app.rescan();
        let names: Vec<&str> = app.library.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a.wav", "b.WAV", "broken.wav", "long.wav"]);
        let summary = |i: usize| app.library[i].summary();
        assert!(summary(0).starts_with("0:02.0"), "{}", summary(0));
        assert!(summary(0).contains("8 kHz mono, 16-bit"), "{}", summary(0));
        assert!(summary(1).contains("stereo"), "{}", summary(1));
        assert!(summary(2).starts_with("Will not open: "), "{}", summary(2));
        assert!(summary(3).starts_with("1:10.0"), "{}", summary(3));
        assert!(app.library_note.is_none());
        let drawn = texts(&app);
        assert!(drawn.iter().any(|t| t == &summary(3)));
        // A folder that is not there yet says the first take will make it.
        app.recordings_dir = Some(dir.0.join("missing"));
        app.rescan();
        assert!(app.library.is_empty());
        assert!(
            app.library_note
                .as_deref()
                .unwrap()
                .contains("does not exist yet")
        );
    }

    /// A recording opens as its whole waveform, drawn from its samples.
    #[test]
    fn a_recording_opens_as_its_whole_waveform() {
        let (_dir, app) = fixture("open");
        let open = app.open.as_ref().unwrap();
        assert_eq!(open.peaks.len(), PEAK_COLUMNS);
        assert!(
            open.peaks.iter().any(|p| p.high > 0.2),
            "the tone is not in the peaks"
        );
        assert_eq!(open.markers.len(), 1);
        assert_eq!(app.panel, Panel::Recording);
        let drawn = texts(&app);
        assert!(drawn.iter().any(|t| t == "a.wav"));
        assert!(
            drawn.iter().any(|t| t == "intro"),
            "the file's marker is not drawn"
        );
        let wave = probe::rect_of(&app, Target::Waveform).unwrap();
        let bars = app
            .frame()
            .commands()
            .iter()
            .filter(|c| matches!(c, RenderCommand::FillRect { width, x, .. } if *width == 1.0 && *x >= wave.x && *x < wave.x + wave.w))
            .count();
        assert!(
            bars as f32 >= wave.w - 1.0,
            "{bars} columns in {} px",
            wave.w
        );
    }

    /// The cursor keys: tenths, seconds, the ends, the markers.
    #[test]
    fn the_cursor_moves_by_the_keys() {
        let (_dir, mut app) = fixture("cursor");
        let at = |app: &SoundRecorderApp| app.open.as_ref().unwrap().cursor;
        app.handle_event(&key(Key::Right));
        assert_eq!(at(&app), 800);
        app.handle_event(&Event::Key(probe::shift(Key::Right)));
        assert_eq!(at(&app), 8_800);
        app.handle_event(&key(Key::End));
        assert_eq!(at(&app), 16_000);
        app.handle_event(&Event::Key(probe::shift(Key::Right)));
        assert_eq!(at(&app), 16_000, "past the end");
        app.handle_event(&Event::Key(probe::ctrl(Key::Left)));
        assert_eq!(at(&app), 2_000, "to the marker before");
        assert_eq!(app.open.as_ref().unwrap().chosen_marker, Some(0));
        app.handle_event(&key(Key::Home));
        assert_eq!(at(&app), 0);
        app.handle_event(&key(Key::Left));
        assert_eq!(at(&app), 0, "past the start");
    }

    /// Markers are put down, named, taken away and saved into the file --
    /// the same samples, the markers the only change.
    #[test]
    fn markers_are_named_removed_and_saved_into_the_file() {
        let (_dir, mut app) = fixture("markers");
        let path = app.open.as_ref().unwrap().path.clone();
        let before = wavpcm::decode(&std::fs::read(&path).unwrap()).unwrap();
        app.handle_event(&key(Key::End));
        app.handle_event(&key(Key::M));
        assert!(app.open.as_ref().unwrap().markers_changed);
        assert!(texts(&app).iter().any(|t| t == "Markers not saved"));
        app.handle_event(&key(Key::F2));
        assert!(app.rename.is_some());
        probe::type_str(&mut app, "Outro");
        app.handle_event(&key(Key::Enter));
        assert!(app.rename.is_none());
        app.handle_event(&Event::Key(probe::ctrl(Key::S)));
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            wavpcm::cues(&bytes).unwrap(),
            vec![
                wavpcm::Cue {
                    frame: 2_000,
                    label: b"intro".to_vec()
                },
                wavpcm::Cue {
                    frame: 16_000,
                    label: b"Outro".to_vec()
                },
            ]
        );
        assert_eq!(
            wavpcm::decode(&bytes).unwrap(),
            before,
            "the samples changed"
        );
        assert!(!app.open.as_ref().unwrap().markers_changed);
        // Delete takes the chosen one away; saved again, the file agrees.
        app.handle_event(&key(Key::Delete));
        app.handle_event(&Event::Key(probe::ctrl(Key::S)));
        assert_eq!(
            wavpcm::cues(&std::fs::read(&path).unwrap()).unwrap().len(),
            1
        );
        // A marker before the others takes its place in frame order.
        app.handle_event(&key(Key::Home));
        app.handle_event(&key(Key::M));
        let open = app.open.as_ref().unwrap();
        assert_eq!(open.markers[0].frame, 0);
        assert_eq!(open.chosen_marker, Some(0));
    }

    /// A file another program wrote since it was opened is not written over
    /// by a save of markers made against the old one.
    #[test]
    fn markers_are_not_saved_over_a_file_changed_on_disk() {
        let (dir, mut app) = fixture("stale");
        let path = app.open.as_ref().unwrap().path.clone();
        std::thread::sleep(Duration::from_millis(20));
        let replacement = std::fs::read(dir.wav("other.wav", 8_000, 1, 0.5, &[])).unwrap();
        std::fs::write(&path, &replacement).unwrap();
        app.handle_event(&key(Key::M));
        assert!(!app.save_markers());
        assert!(
            app.status_line.contains("changed on disk"),
            "{}",
            app.status_line
        );
        assert_eq!(std::fs::read(&path).unwrap(), replacement);
    }

    /// The kept part goes to a new file, its samples copied as stored, and
    /// never over the recording itself.
    #[test]
    fn the_kept_part_is_saved_as_a_file_of_its_own() {
        let (dir, mut app) = fixture("kept");
        let path = app.open.as_ref().unwrap().path.clone();
        let original = std::fs::read(&path).unwrap();
        app.handle_event(&Event::Key(probe::shift(Key::Right)));
        app.handle_event(&key(Key::LeftBracket));
        app.handle_event(&key(Key::Right));
        app.handle_event(&key(Key::Right));
        app.handle_event(&key(Key::RightBracket));
        let kept = app.open.as_ref().unwrap().kept.clone();
        assert_eq!((kept.start_frame, kept.end_frame), (8_000, 9_600));
        // A marker not yet saved into the recording still goes with the part.
        app.handle_event(&key(Key::Left));
        app.handle_event(&key(Key::M));
        app.picker_for = PickerFor::SaveKept;
        let part = dir.0.join("part.wav");
        app.picked(&part);
        let bytes = std::fs::read(&part).unwrap();
        let info = wavpcm::parse_header(&bytes).unwrap();
        assert_eq!(info.frames, 1_600);
        assert_eq!(
            wavpcm::cues(&bytes).unwrap(),
            vec![wavpcm::Cue {
                frame: 800,
                label: b"Marker 1".to_vec()
            }],
            "the part's markers are the ones inside it, moved to its start"
        );
        let src = wavpcm::parse_header(&original).unwrap();
        assert_eq!(
            &bytes[info.data_offset..info.data_offset + info.data_len],
            &original[src.data_offset + 16_000..src.data_offset + 19_200]
        );
        assert!(
            app.library.iter().any(|e| e.path == part),
            "the new file is not listed"
        );
        // Over the recording itself: refused, and the recording untouched.
        app.picked(&path);
        assert!(app.status_line.contains("itself"), "{}", app.status_line);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        app.handle_event(&Event::Key(probe::ctrl(Key::A)));
        assert!(app.open.as_ref().unwrap().kept.is_full());
    }

    /// Unsaved markers are not left behind by one press on another file: the
    /// first asks, the second goes.
    #[test]
    fn unsaved_markers_are_not_left_by_a_single_press() {
        let (_dir, mut app) = fixture("leave");
        app.handle_event(&key(Key::End));
        app.handle_event(&key(Key::M));
        probe::click(&mut app, Target::LibraryRow(1));
        assert_eq!(app.open.as_ref().unwrap().name, "a.wav");
        assert!(app.status_line.contains("not saved"), "{}", app.status_line);
        probe::click(&mut app, Target::LibraryRow(1));
        assert_eq!(app.open.as_ref().unwrap().name, "b.wav");
    }

    /// A marker name that is not UTF-8 is shown escaped, and kept as its
    /// bytes when the file's markers are saved again.
    #[test]
    fn a_marker_name_that_is_not_utf8_is_shown_and_kept() {
        let dir = Scratch::new("bytes");
        let path = dir.wav("x.wav", 8_000, 1, 1.0, &[(10, b"\xE9t\xE9")]);
        let mut app = SoundRecorderApp::new();
        assert!(app.open_path(&path));
        assert!(texts(&app).iter().any(|t| t == "\\xE9t\\xE9"));
        app.handle_event(&key(Key::End));
        app.handle_event(&key(Key::M));
        app.handle_event(&Event::Key(probe::ctrl(Key::S)));
        let cues = wavpcm::cues(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(cues[0].label, b"\xE9t\xE9");
    }

    /// Every control answers the pointer.
    #[test]
    fn every_control_answers_the_pointer() {
        let (_dir, mut app) = fixture("pointer");
        probe::click(&mut app, Target::OpenFile);
        assert!(app.picker.is_open() && app.picker_for == PickerFor::Open);
        app.picker.close();
        probe::click(&mut app, Target::ChooseFolder);
        assert!(app.picker.is_open() && app.picker_for == PickerFor::Folder);
        app.picker.close();
        assert_eq!(
            probe::click(&mut app, Target::Refresh),
            EventResult::Consumed
        );
        probe::click(&mut app, Target::LibraryRow(1));
        assert_eq!(app.open.as_ref().unwrap().name, "b.wav");
        probe::click(&mut app, Target::LibraryRow(0));
        assert_eq!(app.open.as_ref().unwrap().name, "a.wav");
        probe::click(&mut app, Target::Waveform);
        let mid = app.open.as_ref().unwrap().cursor;
        assert!(
            (7_900..=8_100).contains(&mid),
            "the press put the cursor at {mid}"
        );
        probe::click(&mut app, Target::AddMarker);
        assert_eq!(app.open.as_ref().unwrap().markers.len(), 2);
        probe::click(&mut app, Target::MarkerRow(0));
        assert_eq!(app.open.as_ref().unwrap().cursor, 2_000);
        probe::click(&mut app, Target::RenameMarker);
        assert!(app.rename.is_some());
        probe::click(&mut app, Target::RemoveMarker);
        assert!(
            app.rename.is_none(),
            "a press elsewhere keeps the name and ends the field"
        );
        assert_eq!(app.open.as_ref().unwrap().markers.len(), 1);
        probe::click(&mut app, Target::MarkerRow(0));
        probe::click(&mut app, Target::KeepTo);
        probe::click(&mut app, Target::Waveform);
        probe::click(&mut app, Target::KeepFrom);
        let kept = app.open.as_ref().unwrap().kept.clone();
        assert!(!kept.is_full());
        probe::click(&mut app, Target::KeepAll);
        assert!(app.open.as_ref().unwrap().kept.is_full());
        probe::click(&mut app, Target::SaveMarkers);
        assert!(!app.open.as_ref().unwrap().markers_changed);
        assert!(
            probe::rect_of(&app, Target::SaveMarkers).is_none(),
            "nothing left to save"
        );
        probe::click(&mut app, Target::SaveKept);
        assert!(app.picker.is_open() && app.picker_for == PickerFor::SaveKept);
        app.picker.close();
        probe::click(&mut app, Target::Record);
        assert_eq!(app.blocked_reason.as_deref(), Some(CANNOT_RECORD));
        probe::click(&mut app, Target::Help);
        assert!(app.show_help);
        probe::click(&mut app, Target::HelpCard);
        assert!(!app.show_help);
        probe::click(&mut app, Target::Panel(Panel::Library));
        assert_eq!(app.panel, Panel::Library);
    }

    /// A drag on the waveform moves the cursor, a marker, or a handle of the
    /// kept stretch; a release ends it.
    #[test]
    fn the_waveform_is_dragged() {
        let (_dir, mut app) = fixture("drag");
        let wave = probe::rect_of(&app, Target::Waveform).unwrap();
        let x_of = |f: u64| frame_x(wave, f, 16_000);
        let y = wave.y + wave.h / 2.0;
        // The marker at 2,000: chosen by the press, moved by the drag.
        mouse(
            &mut app,
            x_of(2_000),
            y,
            MouseEventKind::Press(MouseButton::Left),
        );
        assert_eq!(app.open.as_ref().unwrap().chosen_marker, Some(0));
        mouse(&mut app, x_of(2_000) + 1.0, y, MouseEventKind::Move);
        assert_eq!(
            app.open.as_ref().unwrap().markers[0].frame,
            2_000,
            "a shake moved it"
        );
        mouse(&mut app, x_of(6_000), y, MouseEventKind::Move);
        let moved = app.open.as_ref().unwrap().markers[0].frame;
        assert!((5_950..=6_050).contains(&moved), "{moved}");
        mouse(
            &mut app,
            x_of(6_000),
            y,
            MouseEventKind::Release(MouseButton::Left),
        );
        mouse(&mut app, x_of(9_000), y, MouseEventKind::Move);
        assert_eq!(
            app.open.as_ref().unwrap().markers[0].frame,
            moved,
            "the drag outlived the release"
        );
        // The kept stretch's start handle, from 0.
        mouse(
            &mut app,
            wave.x + 1.0,
            y,
            MouseEventKind::Press(MouseButton::Left),
        );
        mouse(&mut app, x_of(4_000), y, MouseEventKind::Move);
        mouse(
            &mut app,
            x_of(4_000),
            y,
            MouseEventKind::Release(MouseButton::Left),
        );
        let start = app.open.as_ref().unwrap().kept.start_frame;
        assert!((3_950..=4_050).contains(&start), "{start}");
        // Anywhere else the cursor follows.
        mouse(
            &mut app,
            x_of(10_000),
            y,
            MouseEventKind::Press(MouseButton::Left),
        );
        mouse(&mut app, x_of(12_000), y, MouseEventKind::Move);
        let cursor = app.open.as_ref().unwrap().cursor;
        assert!((11_950..=12_050).contains(&cursor), "{cursor}");
    }

    /// The recordings list scrolls under the wheel and follows the keys.
    #[test]
    fn the_list_scrolls_and_follows_the_keys() {
        let dir = Scratch::new("scroll");
        for i in 0..40 {
            dir.wav(&format!("take{i:02}.wav"), 8_000, 1, 0.01, &[]);
        }
        let mut app = SoundRecorderApp::new();
        app.recordings_dir = Some(dir.0.clone());
        app.rescan();
        let (_, rows) = app.library_pane();
        assert!(rows < 40);
        for _ in 0..30 {
            probe::scroll_at_point(&mut app, Target::LibraryList, -3.0);
        }
        assert_eq!(app.library_scroll, 40 - rows);
        app.render(app.window_width, app.window_height);
        assert_eq!(app.library_scroll, 40 - rows, "a frame moved the list");
        app.panel = Panel::Library;
        app.handle_event(&key(Key::Down));
        assert_eq!(
            app.chosen_entry.as_deref(),
            Some(dir.0.join("take00.wav").as_path())
        );
        assert_eq!(app.library_scroll, 0, "the choice was left off screen");
        app.handle_event(&key(Key::Enter));
        assert_eq!(app.open.as_ref().unwrap().name, "take00.wav");
    }

    /// The window draws in the user's colours rather than constants of its
    /// own.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        fn theme(
            mode: appearance::ThemeMode,
            contrast: Option<appearance::HighContrastScheme>,
        ) -> Palette {
            Palette::from_settings(&appearance::AppearanceSettings {
                theme_mode: mode,
                high_contrast: contrast,
                ..appearance::AppearanceSettings::default()
            })
        }
        fn fills(app: &mut SoundRecorderApp) -> Vec<Color> {
            oswindow::app::App::render(app, 800.0, 600.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }
        let mut app = SoundRecorderApp::new();
        oswindow::app::App::theme_changed(&mut app, &theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty());
        oswindow::app::App::theme_changed(&mut app, &theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(dark, light);
        oswindow::app::App::theme_changed(
            &mut app,
            &theme(
                appearance::ThemeMode::Dark,
                Some(appearance::HighContrastScheme::WhiteOnBlack),
            ),
        );
        assert_ne!(dark, fills(&mut app));
    }
}
