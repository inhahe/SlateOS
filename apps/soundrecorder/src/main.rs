//! OurOS Sound Recorder
//!
//! A sound recording utility providing WAV capture with real-time waveform
//! visualization, VU metering, markers, trim tool, playback, and a file
//! browser for saved recordings. Uses the guitk library for UI rendering
//! with Catppuccin Mocha theme.

#![allow(dead_code)]

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::collections::VecDeque;

// ============================================================================
// Catppuccin Mocha color palette
// ============================================================================

mod colors {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const MANTLE: Color = Color::from_hex(0x181825);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
}

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
    pub fn color(self) -> Color {
        match self {
            Self::Idle => colors::SUBTEXT0,
            Self::Recording => colors::RED,
            Self::Paused => colors::YELLOW,
            Self::Stopped => colors::GREEN,
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
        self.hz() * 2 // 16 bits = 2 bytes per sample
    }

    /// Bytes per second for stereo 16-bit PCM at this rate.
    pub fn bytes_per_second_stereo(self) -> u32 {
        self.hz() * 4 // 2 channels * 2 bytes per sample
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
        self.sample_rate().hz() * u32::from(self.channels()) * sample_bytes
    }
}

// ============================================================================
// Audio input device
// ============================================================================

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
        self.samples.len() / ch
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
        self.channels * (self.bits_per_sample / 8)
    }

    /// Byte rate: sample_rate * block_align.
    pub fn byte_rate(&self) -> u32 {
        self.sample_rate * u32::from(self.block_align())
    }

    /// Size of the raw PCM data in bytes.
    pub fn data_size(&self) -> u32 {
        (self.samples.len() * 2) as u32
    }

    /// Generate the complete WAV file as a byte vector.
    ///
    /// Layout:
    /// - RIFF header (12 bytes)
    /// - fmt  sub-chunk (24 bytes)
    /// - data sub-chunk header (8 bytes) + raw PCM data
    pub fn to_bytes(&self) -> Vec<u8> {
        let data_size = self.data_size();
        // Total RIFF chunk size = 4 (WAVE) + 24 (fmt) + 8 (data header) + data_size
        let riff_size = 4 + 24 + 8 + data_size;

        let mut buf = Vec::with_capacity(44 + data_size as usize);

        // RIFF header
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&riff_size.to_le_bytes());
        buf.extend_from_slice(b"WAVE");

        // fmt sub-chunk
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes()); // sub-chunk size
        buf.extend_from_slice(&1u16.to_le_bytes()); // PCM format
        buf.extend_from_slice(&self.channels.to_le_bytes());
        buf.extend_from_slice(&self.sample_rate.to_le_bytes());
        buf.extend_from_slice(&self.byte_rate().to_le_bytes());
        buf.extend_from_slice(&self.block_align().to_le_bytes());
        buf.extend_from_slice(&self.bits_per_sample.to_le_bytes());

        // data sub-chunk
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_size.to_le_bytes());
        for &sample in &self.samples {
            buf.extend_from_slice(&sample.to_le_bytes());
        }

        buf
    }

    /// Parse a WAV file from bytes. Returns `None` on invalid data.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 44 {
            return None;
        }
        // Validate RIFF header
        if &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
            return None;
        }
        // Validate fmt sub-chunk
        if &data[12..16] != b"fmt " {
            return None;
        }
        let format_tag = u16::from_le_bytes([data[20], data[21]]);
        if format_tag != 1 {
            return None; // Only PCM supported
        }
        let channels = u16::from_le_bytes([data[22], data[23]]);
        let sample_rate = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
        let bits_per_sample = u16::from_le_bytes([data[34], data[35]]);

        // Find data sub-chunk
        if &data[36..40] != b"data" {
            return None;
        }
        let data_size = u32::from_le_bytes([data[40], data[41], data[42], data[43]]) as usize;

        if data.len() < 44 + data_size {
            return None;
        }

        let pcm_data = &data[44..44 + data_size];
        let mut samples = Vec::with_capacity(data_size / 2);
        let mut i = 0;
        while i + 1 < pcm_data.len() {
            samples.push(i16::from_le_bytes([pcm_data[i], pcm_data[i + 1]]));
            i += 2;
        }

        Some(Self {
            sample_rate,
            channels,
            bits_per_sample,
            samples,
        })
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
        let peak = samples.iter().map(|&s| (s as f32).abs()).fold(0.0f32, f32::max);
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
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut commands = Vec::new();

        // Background
        commands.push(RenderCommand::FillRect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            color: colors::MANTLE,
            corner_radii: CornerRadii::all(4.0),
        });

        // Center line
        let center_y = self.y + self.height / 2.0;
        commands.push(RenderCommand::Line {
            x1: self.x,
            y1: center_y,
            x2: self.x + self.width,
            y2: center_y,
            color: colors::OVERLAY0,
            width: 1.0,
        });

        // Waveform bars
        let col_count = self.amplitudes.len();
        if col_count > 0 {
            let bar_width = self.width / self.max_columns as f32;
            let max_bar_height = self.height / 2.0 - 2.0;
            let start_offset = (self.max_columns - col_count) as f32 * bar_width;

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
                    color: colors::GREEN,
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
            color: colors::SURFACE0,
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
            self.peak_hold_ticks -= 1;
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
    fn level_color(level: f32) -> Color {
        if level < 0.6 {
            colors::GREEN
        } else if level < 0.85 {
            colors::YELLOW
        } else {
            colors::RED
        }
    }

    /// Render the VU meter.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut commands = Vec::new();

        // Background
        commands.push(RenderCommand::FillRect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            color: colors::MANTLE,
            corner_radii: CornerRadii::all(3.0),
        });

        // Level bar
        let bar_width = self.current_level * (self.width - 4.0);
        if bar_width > 0.5 {
            commands.push(RenderCommand::FillRect {
                x: self.x + 2.0,
                y: self.y + 2.0,
                width: bar_width,
                height: self.height - 4.0,
                color: Self::level_color(self.current_level),
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
                color: colors::RED,
                width: 2.0,
            });
        }

        // Border
        commands.push(RenderCommand::StrokeRect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            color: colors::SURFACE0,
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
    /// Available disk space in bytes (for remaining-time estimate).
    available_bytes: u64,
    /// Current bytes-per-second rate for space estimation.
    bytes_per_second: u32,
}

impl RecordingTimer {
    pub fn new(available_bytes: u64, bytes_per_second: u32) -> Self {
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
    pub fn remaining_secs(&self) -> f64 {
        if self.bytes_per_second == 0 {
            return f64::INFINITY;
        }
        self.available_bytes as f64 / self.bytes_per_second as f64
    }

    /// Format remaining time as a human-readable string.
    pub fn format_remaining(&self) -> String {
        let secs = self.remaining_secs();
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
        self.available_bytes = bytes;
    }

    /// Update the byte rate (e.g., after changing quality preset).
    pub fn set_bytes_per_second(&mut self, bps: u32) {
        self.bytes_per_second = bps;
    }

    /// Render the timer display.
    pub fn render(&self, x: f32, y: f32) -> Vec<RenderCommand> {
        let elapsed_text = self.format_elapsed();
        let remaining_text = format!("-{}", self.format_remaining());

        vec![
            // Elapsed time (large)
            RenderCommand::Text {
                x,
                y,
                text: elapsed_text,
                color: colors::TEXT,
                font_size: 28.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            },
            // Remaining label
            RenderCommand::Text {
                x: x + 200.0,
                y: y + 6.0,
                text: remaining_text,
                color: colors::SUBTEXT0,
                font_size: 16.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
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
        self.next_id += 1;
        let color_index = id as usize % 4;
        let color = match color_index {
            0 => colors::BLUE,
            1 => colors::PEACH,
            2 => colors::YELLOW,
            _ => colors::GREEN,
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
        self.end_frame = clamped.max(self.start_frame + 1);
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
        let start_idx = self.start_frame as usize * ch;
        let end_idx = (self.end_frame as usize * ch).min(samples.len());
        if start_idx >= samples.len() || start_idx >= end_idx {
            return Vec::new();
        }
        samples[start_idx..end_idx].to_vec()
    }

    /// Render the trim handles on a waveform region.
    pub fn render(
        &self,
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
            color: colors::BLUE,
            corner_radii: CornerRadii::ZERO,
        });

        // End handle
        commands.push(RenderCommand::FillRect {
            x: end_x - 3.0,
            y: region_y,
            width: 6.0,
            height: region_height,
            color: colors::BLUE,
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
                self.release_counter -= 1;
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
    pub fn render(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut commands = Vec::new();

        // Label
        commands.push(RenderCommand::Text {
            x,
            y,
            text: format!("Noise Gate: {:.0}%", self.threshold * 100.0),
            color: if self.enabled {
                colors::TEXT
            } else {
                colors::OVERLAY0
            },
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Threshold slider track
        let track_y = y + 18.0;
        commands.push(RenderCommand::FillRect {
            x,
            y: track_y,
            width,
            height: 6.0,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });

        // Threshold position
        let knob_x = x + self.threshold * width;
        commands.push(RenderCommand::FillRect {
            x: knob_x - 4.0,
            y: track_y - 4.0,
            width: 8.0,
            height: 14.0,
            color: if self.enabled {
                colors::PEACH
            } else {
                colors::OVERLAY0
            },
            corner_radii: CornerRadii::all(4.0),
        });

        // Gate status indicator
        let status_color = if !self.enabled {
            colors::OVERLAY0
        } else if self.is_open {
            colors::GREEN
        } else {
            colors::RED
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
        let new_pos = (self.position_frames as i64 + delta_frames).max(0) as u64;
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
        let secs = (self.position_frames / self.sample_rate as u64) as u32;
        let minutes = secs / 60;
        let seconds = secs % 60;
        format!("{minutes:02}:{seconds:02}")
    }

    /// Total duration formatted as MM:SS.
    pub fn format_duration(&self) -> String {
        if self.sample_rate == 0 {
            return "00:00".into();
        }
        let secs = (self.total_frames / self.sample_rate as u64) as u32;
        let minutes = secs / 60;
        let seconds = secs % 60;
        format!("{minutes:02}:{seconds:02}")
    }

    /// Render the playback bar.
    pub fn render(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut commands = Vec::new();
        let bar_height = 6.0;
        let bar_y = y + 10.0;

        // Track background
        commands.push(RenderCommand::FillRect {
            x,
            y: bar_y,
            width,
            height: bar_height,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });

        // Progress fill
        let fill_width = self.progress() * width;
        if fill_width > 0.5 {
            commands.push(RenderCommand::FillRect {
                x,
                y: bar_y,
                width: fill_width,
                height: bar_height,
                color: colors::BLUE,
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
            color: colors::TEXT,
            corner_radii: CornerRadii::all(5.0),
        });

        // Time labels
        commands.push(RenderCommand::Text {
            x,
            y: bar_y + bar_height + 6.0,
            text: self.format_position(),
            color: colors::SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        commands.push(RenderCommand::Text {
            x: x + width - 40.0,
            y: bar_y + bar_height + 6.0,
            text: self.format_duration(),
            color: colors::SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
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
            interval_ms: interval_secs as u64 * 1000,
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
            self.elapsed_since_save_ms -= self.interval_ms;
            self.save_count += 1;
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
        self.interval_ms = secs as u64 * 1000;
        self.enabled = secs > 0;
    }
}

// ============================================================================
// Recording entry (file browser / history)
// ============================================================================

/// An entry in the recording history/file browser.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordingEntry {
    /// Unique identifier.
    pub id: u32,
    /// File name.
    pub filename: String,
    /// File path.
    pub path: String,
    /// Duration in seconds.
    pub duration_secs: f64,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Sample rate.
    pub sample_rate: u32,
    /// Number of channels.
    pub channels: u16,
    /// Timestamp (seconds since epoch).
    pub created_timestamp: u64,
}

impl RecordingEntry {
    /// Format the duration as MM:SS.
    pub fn format_duration(&self) -> String {
        let total_secs = self.duration_secs as u32;
        let minutes = total_secs / 60;
        let seconds = total_secs % 60;
        format!("{minutes:02}:{seconds:02}")
    }

    /// Format the file size in human-readable form.
    pub fn format_size(&self) -> String {
        if self.size_bytes < 1024 {
            format!("{} B", self.size_bytes)
        } else if self.size_bytes < 1024 * 1024 {
            format!("{:.1} KB", self.size_bytes as f64 / 1024.0)
        } else {
            format!("{:.1} MB", self.size_bytes as f64 / (1024.0 * 1024.0))
        }
    }
}

/// Recording history list.
pub struct RecordingHistory {
    entries: Vec<RecordingEntry>,
    next_id: u32,
    /// Index of the currently selected entry, if any.
    pub selected: Option<usize>,
}

impl RecordingHistory {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 0,
            selected: None,
        }
    }

    /// Add a recording entry.
    pub fn add(&mut self, entry: RecordingEntry) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let mut e = entry;
        e.id = id;
        self.entries.push(e);
        id
    }

    /// Remove an entry by id.
    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        if self.entries.len() < before {
            // Fix selected index
            if let Some(sel) = self.selected
                && sel >= self.entries.len() {
                    self.selected = if self.entries.is_empty() {
                        None
                    } else {
                        Some(self.entries.len() - 1)
                    };
                }
            true
        } else {
            false
        }
    }

    /// Get all entries.
    pub fn entries(&self) -> &[RecordingEntry] {
        &self.entries
    }

    /// Get entry by id.
    pub fn get(&self, id: u32) -> Option<&RecordingEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Get the selected entry.
    pub fn selected_entry(&self) -> Option<&RecordingEntry> {
        self.selected.and_then(|i| self.entries.get(i))
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the history is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Select the next entry.
    pub fn select_next(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = Some(match self.selected {
            Some(i) if i + 1 < self.entries.len() => i + 1,
            _ => 0,
        });
    }

    /// Select the previous entry.
    pub fn select_prev(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = Some(match self.selected {
            Some(0) | None => self.entries.len().saturating_sub(1),
            Some(i) => i - 1,
        });
    }

    /// Render the recording history list.
    pub fn render(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut commands = Vec::new();
        let row_height = 36.0;

        // Header
        commands.push(RenderCommand::Text {
            x,
            y,
            text: "Recording History".into(),
            color: colors::TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        if self.entries.is_empty() {
            commands.push(RenderCommand::Text {
                x,
                y: y + 24.0,
                text: "No recordings yet.".into(),
                color: colors::OVERLAY0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            return commands;
        }

        for (i, entry) in self.entries.iter().enumerate() {
            let ey = y + 24.0 + i as f32 * row_height;
            let is_selected = self.selected == Some(i);

            // Row background
            if is_selected {
                commands.push(RenderCommand::FillRect {
                    x,
                    y: ey,
                    width,
                    height: row_height - 2.0,
                    color: colors::SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            // Filename
            commands.push(RenderCommand::Text {
                x: x + 8.0,
                y: ey + 4.0,
                text: entry.filename.clone(),
                color: if is_selected {
                    colors::BLUE
                } else {
                    colors::TEXT
                },
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.5),
            });

            // Duration and size
            let info = format!("{} | {}", entry.format_duration(), entry.format_size());
            commands.push(RenderCommand::Text {
                x: x + 8.0,
                y: ey + 19.0,
                text: info,
                color: colors::SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        commands
    }
}

impl Default for RecordingHistory {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Main application state
// ============================================================================

/// Top-level application state for the sound recorder.
pub struct SoundRecorderApp {
    /// Current recording state.
    pub state: RecordingState,
    /// Selected quality preset.
    pub preset: QualityPreset,
    /// Selected sample rate (can override preset).
    pub sample_rate: SampleRate,
    /// Available input devices.
    pub input_devices: Vec<AudioInputDevice>,
    /// Index of the selected input device.
    pub selected_device: usize,
    /// WAV file being recorded.
    pub wav: WavFile,
    /// Waveform display.
    pub waveform: WaveformDisplay,
    /// VU meter.
    pub vu_meter: VuMeter,
    /// Recording timer.
    pub timer: RecordingTimer,
    /// Markers placed during recording.
    pub markers: MarkerList,
    /// Trim region for the current recording.
    pub trim: Option<TrimRegion>,
    /// Noise gate.
    pub noise_gate: NoiseGate,
    /// Playback controller.
    pub playback: PlaybackController,
    /// Auto-save manager.
    pub auto_save: AutoSave,
    /// Recording history.
    pub history: RecordingHistory,
    /// Window dimensions.
    pub window_width: f32,
    pub window_height: f32,
}

impl SoundRecorderApp {
    /// Create a new sound recorder application with default settings.
    pub fn new() -> Self {
        let preset = QualityPreset::Music;
        Self {
            state: RecordingState::Idle,
            preset,
            sample_rate: preset.sample_rate(),
            input_devices: AudioInputDevice::mock_devices(),
            selected_device: 0,
            wav: WavFile::from_preset(preset),
            waveform: WaveformDisplay::new(20.0, 120.0, 560.0, 100.0),
            vu_meter: VuMeter::new(20.0, 230.0, 560.0, 20.0),
            timer: RecordingTimer::new(1_000_000_000, preset.bytes_per_second()),
            markers: MarkerList::new(),
            trim: None,
            noise_gate: NoiseGate::new(0.02),
            playback: PlaybackController::new(),
            auto_save: AutoSave::new(60),
            history: RecordingHistory::new(),
            window_width: 600.0,
            window_height: 500.0,
        }
    }

    /// Transition to a new recording state if the transition is valid.
    /// Returns true if the transition was performed.
    pub fn transition_to(&mut self, target: RecordingState) -> bool {
        if !self.state.can_transition_to(target) {
            return false;
        }

        match target {
            RecordingState::Recording => {
                if self.state == RecordingState::Idle {
                    // Starting new recording: reset everything
                    self.wav = WavFile::from_preset(self.preset);
                    self.waveform.clear();
                    self.vu_meter.reset();
                    self.timer.reset();
                    self.markers.clear();
                    self.trim = None;
                    self.noise_gate.reset();
                    self.auto_save.reset_timer();
                }
                // Paused -> Recording is a resume: just change state
            }
            RecordingState::Stopped => {
                // Set up trim region and playback for the finished recording
                let frame_count = self.wav.frame_count() as u64;
                self.trim = Some(TrimRegion::full(frame_count));
                self.playback.load(frame_count, self.wav.sample_rate);
            }
            RecordingState::Idle => {
                // Reset from Stopped back to Idle
                self.trim = None;
            }
            RecordingState::Paused => {}
        }

        self.state = target;
        true
    }

    /// Set the quality preset and update related settings.
    pub fn set_preset(&mut self, preset: QualityPreset) {
        self.preset = preset;
        self.sample_rate = preset.sample_rate();
        self.timer
            .set_bytes_per_second(preset.bytes_per_second());
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

        // Apply noise gate
        self.noise_gate.process(&mut processed);

        // Add to WAV data
        self.wav.push_samples(&processed);

        // Update waveform display
        let amplitude = WaveformDisplay::amplitude_from_samples(samples);
        self.waveform.push_amplitude(amplitude);

        // Update VU meter
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

    /// Save the current recording (with optional trim) to the history.
    pub fn save_recording(&mut self, filename: String) -> Option<u32> {
        if self.state != RecordingState::Stopped {
            return None;
        }

        let wav_data = if let Some(ref trim) = self.trim {
            let trimmed_samples = trim.apply(&self.wav.samples, self.wav.channels);
            let mut trimmed_wav = WavFile::new(
                self.wav.sample_rate,
                self.wav.channels,
                self.wav.bits_per_sample,
            );
            trimmed_wav.push_samples(&trimmed_samples);
            trimmed_wav.to_bytes()
        } else {
            self.wav.to_bytes()
        };

        let entry = RecordingEntry {
            id: 0, // Will be set by history.add()
            filename: filename.clone(),
            path: format!("/recordings/{filename}"),
            duration_secs: self.wav.duration_secs(),
            size_bytes: wav_data.len() as u64,
            sample_rate: self.wav.sample_rate,
            channels: self.wav.channels,
            created_timestamp: 0,
        };

        Some(self.history.add(entry))
    }

    /// Render the full application UI.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Window background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: colors::BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title bar
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: 40.0,
            color: colors::MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: 10.0,
            text: "Sound Recorder".into(),
            color: colors::TEXT,
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // State indicator
        cmds.push(RenderCommand::FillRect {
            x: 160.0,
            y: 12.0,
            width: 10.0,
            height: 10.0,
            color: self.state.color(),
            corner_radii: CornerRadii::all(5.0),
        });
        cmds.push(RenderCommand::Text {
            x: 176.0,
            y: 10.0,
            text: self.state.label().into(),
            color: self.state.color(),
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Device selector area
        if let Some(device) = self.current_device() {
            cmds.push(RenderCommand::Text {
                x: 20.0,
                y: 50.0,
                text: format!("Input: {}", device.name),
                color: colors::SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(300.0),
            });
        }

        // Preset label
        cmds.push(RenderCommand::Text {
            x: 350.0,
            y: 50.0,
            text: format!("Quality: {}", self.preset.label()),
            color: colors::SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(240.0),
        });

        // Timer display
        cmds.extend(self.timer.render(20.0, 75.0));

        // Waveform
        cmds.extend(self.waveform.render());

        // VU meter
        cmds.extend(self.vu_meter.render());

        // Markers overlay on waveform
        let wf = &self.waveform;
        cmds.extend(self.markers.render(
            self.wav.frame_count() as u64,
            wf.x,
            wf.y,
            wf.width,
            wf.height,
        ));

        // Trim handles when stopped
        if let Some(ref trim) = self.trim {
            cmds.extend(trim.render(wf.x, wf.y, wf.width, wf.height));
        }

        // Noise gate control
        cmds.extend(self.noise_gate.render(20.0, 260.0, 200.0));

        // Playback bar when stopped
        if self.state == RecordingState::Stopped {
            cmds.extend(self.playback.render(20.0, 300.0, 560.0));
        }

        // Control buttons
        let button_y = 340.0;
        self.render_controls(&mut cmds, 20.0, button_y);

        // Recording history (right side or below)
        cmds.extend(self.history.render(20.0, 390.0, 560.0));

        cmds
    }

    /// Render control buttons based on the current state.
    fn render_controls(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) {
        match self.state {
            RecordingState::Idle => {
                self.render_button(cmds, x, y, 100.0, 32.0, "Record", colors::RED);
            }
            RecordingState::Recording => {
                self.render_button(cmds, x, y, 80.0, 32.0, "Pause", colors::YELLOW);
                self.render_button(cmds, x + 90.0, y, 80.0, 32.0, "Stop", colors::PEACH);
                self.render_button(
                    cmds,
                    x + 180.0,
                    y,
                    100.0,
                    32.0,
                    "Marker",
                    colors::BLUE,
                );
            }
            RecordingState::Paused => {
                self.render_button(cmds, x, y, 80.0, 32.0, "Resume", colors::GREEN);
                self.render_button(cmds, x + 90.0, y, 80.0, 32.0, "Stop", colors::PEACH);
                self.render_button(
                    cmds,
                    x + 180.0,
                    y,
                    100.0,
                    32.0,
                    "Marker",
                    colors::BLUE,
                );
            }
            RecordingState::Stopped => {
                self.render_button(cmds, x, y, 80.0, 32.0, "New", colors::GREEN);
                self.render_button(cmds, x + 90.0, y, 80.0, 32.0, "Save", colors::BLUE);
                self.render_button(cmds, x + 180.0, y, 80.0, 32.0, "Play", colors::PEACH);
            }
        }
    }

    /// Render a single button.
    fn render_button(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        label: &str,
        color: Color,
    ) {
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 10.0,
            y: y + 8.0,
            text: label.into(),
            color: colors::BASE,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
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

fn main() {
    // Placeholder: the actual event loop will be provided by the OS
    // windowing system. For now this just validates that the application
    // compiles and the types are wired together correctly.
    let _app = SoundRecorderApp::new();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- RecordingState tests ------------------------------------------------

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
        let idle = RecordingState::Idle.color();
        let recording = RecordingState::Recording.color();
        let paused = RecordingState::Paused.color();
        let stopped = RecordingState::Stopped.color();
        assert_ne!(idle, recording);
        assert_ne!(recording, paused);
        assert_ne!(paused, stopped);
    }

    // -- SampleRate tests ----------------------------------------------------

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

    // -- QualityPreset tests -------------------------------------------------

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

    // -- AudioInputDevice tests -----------------------------------------------

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

    // -- WavFile tests -------------------------------------------------------

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
    fn test_wav_to_bytes_header() {
        let wav = WavFile::new(44100, 1, 16);
        let bytes = wav.to_bytes();
        assert!(bytes.len() >= 44);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(&bytes[36..40], b"data");
    }

    #[test]
    fn test_wav_roundtrip() {
        let mut wav = WavFile::new(48000, 2, 16);
        wav.push_samples(&[1000, -2000, 3000, -4000, 5000, -6000]);
        let bytes = wav.to_bytes();
        let parsed = WavFile::from_bytes(&bytes).expect("parse failed");
        assert_eq!(parsed.sample_rate, 48000);
        assert_eq!(parsed.channels, 2);
        assert_eq!(parsed.bits_per_sample, 16);
        assert_eq!(parsed.samples, wav.samples);
    }

    #[test]
    fn test_wav_parse_invalid_too_short() {
        assert!(WavFile::from_bytes(&[0; 10]).is_none());
    }

    #[test]
    fn test_wav_parse_invalid_header() {
        let mut data = vec![0u8; 44];
        data[0..4].copy_from_slice(b"NOPE");
        assert!(WavFile::from_bytes(&data).is_none());
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

    // -- WaveformDisplay tests -----------------------------------------------

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
        let mut wf = WaveformDisplay::new(0.0, 0.0, 100.0, 50.0);
        wf.push_amplitude(0.5);
        let cmds = wf.render();
        assert!(!cmds.is_empty());
    }

    // -- VuMeter tests -------------------------------------------------------

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
        let vu = VuMeter::new(0.0, 0.0, 100.0, 20.0);
        let cmds = vu.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_vu_meter_level_color_ranges() {
        assert_eq!(VuMeter::level_color(0.3), colors::GREEN);
        assert_eq!(VuMeter::level_color(0.7), colors::YELLOW);
        assert_eq!(VuMeter::level_color(0.95), colors::RED);
    }

    // -- RecordingTimer tests ------------------------------------------------

    #[test]
    fn test_timer_initial_zero() {
        let t = RecordingTimer::new(1_000_000, 192000);
        assert_eq!(t.elapsed_secs(), 0.0);
    }

    #[test]
    fn test_timer_tick() {
        let mut t = RecordingTimer::new(1_000_000, 192000);
        t.tick(1500);
        assert!((t.elapsed_secs() - 1.5).abs() < 0.001);
    }

    #[test]
    fn test_timer_format_elapsed() {
        let mut t = RecordingTimer::new(0, 0);
        t.tick(3661_000); // 1h 1m 1s
        assert_eq!(t.format_elapsed(), "01:01:01");
    }

    #[test]
    fn test_timer_remaining() {
        let t = RecordingTimer::new(384_000, 192000);
        assert!((t.remaining_secs() - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_timer_remaining_zero_rate() {
        let t = RecordingTimer::new(1000, 0);
        assert!(t.remaining_secs().is_infinite());
    }

    #[test]
    fn test_timer_format_remaining_overflow() {
        let t = RecordingTimer::new(u64::MAX, 1);
        assert_eq!(t.format_remaining(), "99:59:59+");
    }

    #[test]
    fn test_timer_reset() {
        let mut t = RecordingTimer::new(1000, 100);
        t.tick(5000);
        t.reset();
        assert_eq!(t.elapsed_secs(), 0.0);
    }

    #[test]
    fn test_timer_render_produces_commands() {
        let t = RecordingTimer::new(1000, 100);
        let cmds = t.render(0.0, 0.0);
        assert!(!cmds.is_empty());
    }

    // -- MarkerList tests ----------------------------------------------------

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

    // -- TrimRegion tests ----------------------------------------------------

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
        let t = TrimRegion::full(1000);
        let cmds = t.render(0.0, 0.0, 100.0, 50.0);
        // Should have 2 handle rects but no dim rects
        assert_eq!(cmds.len(), 2);
    }

    #[test]
    fn test_trim_render_partial_has_dim() {
        let mut t = TrimRegion::full(1000);
        t.set_start(200);
        t.set_end(800);
        let cmds = t.render(0.0, 0.0, 100.0, 50.0);
        // 2 dim regions + 2 handles = 4
        assert_eq!(cmds.len(), 4);
    }

    // -- NoiseGate tests -----------------------------------------------------

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
        let ng = NoiseGate::new(0.1);
        let cmds = ng.render(0.0, 0.0, 100.0);
        assert!(!cmds.is_empty());
    }

    // -- PlaybackController tests --------------------------------------------

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
        let pb = PlaybackController::new();
        let cmds = pb.render(0.0, 0.0, 100.0);
        assert!(!cmds.is_empty());
    }

    // -- AutoSave tests ------------------------------------------------------

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

    // -- RecordingEntry tests ------------------------------------------------

    #[test]
    fn test_entry_format_duration() {
        let e = RecordingEntry {
            id: 0,
            filename: "test.wav".into(),
            path: "/test.wav".into(),
            duration_secs: 125.0,
            size_bytes: 0,
            sample_rate: 48000,
            channels: 2,
            created_timestamp: 0,
        };
        assert_eq!(e.format_duration(), "02:05");
    }

    #[test]
    fn test_entry_format_size_bytes() {
        let e = RecordingEntry {
            id: 0, filename: "x".into(), path: "x".into(),
            duration_secs: 0.0, size_bytes: 512,
            sample_rate: 48000, channels: 1, created_timestamp: 0,
        };
        assert_eq!(e.format_size(), "512 B");
    }

    #[test]
    fn test_entry_format_size_kb() {
        let e = RecordingEntry {
            id: 0, filename: "x".into(), path: "x".into(),
            duration_secs: 0.0, size_bytes: 2048,
            sample_rate: 48000, channels: 1, created_timestamp: 0,
        };
        assert_eq!(e.format_size(), "2.0 KB");
    }

    #[test]
    fn test_entry_format_size_mb() {
        let e = RecordingEntry {
            id: 0, filename: "x".into(), path: "x".into(),
            duration_secs: 0.0, size_bytes: 5_242_880,
            sample_rate: 48000, channels: 1, created_timestamp: 0,
        };
        assert_eq!(e.format_size(), "5.0 MB");
    }

    // -- RecordingHistory tests ----------------------------------------------

    #[test]
    fn test_history_empty() {
        let h = RecordingHistory::new();
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
        assert!(h.selected_entry().is_none());
    }

    #[test]
    fn test_history_add_and_get() {
        let mut h = RecordingHistory::new();
        let entry = RecordingEntry {
            id: 0, filename: "rec1.wav".into(), path: "/rec1.wav".into(),
            duration_secs: 10.0, size_bytes: 1000,
            sample_rate: 48000, channels: 2, created_timestamp: 0,
        };
        let id = h.add(entry);
        assert_eq!(h.len(), 1);
        assert!(h.get(id).is_some());
    }

    #[test]
    fn test_history_remove() {
        let mut h = RecordingHistory::new();
        let entry = RecordingEntry {
            id: 0, filename: "x.wav".into(), path: "/x.wav".into(),
            duration_secs: 1.0, size_bytes: 100,
            sample_rate: 48000, channels: 1, created_timestamp: 0,
        };
        let id = h.add(entry);
        assert!(h.remove(id));
        assert!(h.is_empty());
    }

    #[test]
    fn test_history_select_next_wrap() {
        let mut h = RecordingHistory::new();
        for i in 0..3 {
            h.add(RecordingEntry {
                id: 0, filename: format!("rec{i}.wav"), path: format!("/rec{i}.wav"),
                duration_secs: 1.0, size_bytes: 100,
                sample_rate: 48000, channels: 1, created_timestamp: 0,
            });
        }
        h.select_next(); // -> 0
        h.select_next(); // -> 1
        h.select_next(); // -> 2
        h.select_next(); // -> wrap to 0
        assert_eq!(h.selected, Some(0));
    }

    #[test]
    fn test_history_select_prev() {
        let mut h = RecordingHistory::new();
        for i in 0..3 {
            h.add(RecordingEntry {
                id: 0, filename: format!("rec{i}.wav"), path: format!("/rec{i}.wav"),
                duration_secs: 1.0, size_bytes: 100,
                sample_rate: 48000, channels: 1, created_timestamp: 0,
            });
        }
        h.select_prev(); // None -> last (2)
        assert_eq!(h.selected, Some(2));
        h.select_prev(); // 2 -> 1
        assert_eq!(h.selected, Some(1));
    }

    // -- SoundRecorderApp tests ----------------------------------------------

    #[test]
    fn test_app_creation() {
        let app = SoundRecorderApp::new();
        assert_eq!(app.state, RecordingState::Idle);
        assert_eq!(app.preset, QualityPreset::Music);
    }

    #[test]
    fn test_app_start_recording() {
        let mut app = SoundRecorderApp::new();
        assert!(app.transition_to(RecordingState::Recording));
        assert_eq!(app.state, RecordingState::Recording);
    }

    #[test]
    fn test_app_full_lifecycle() {
        let mut app = SoundRecorderApp::new();
        assert!(app.transition_to(RecordingState::Recording));
        assert!(app.transition_to(RecordingState::Paused));
        assert!(app.transition_to(RecordingState::Recording)); // resume
        assert!(app.transition_to(RecordingState::Stopped));
        assert!(app.transition_to(RecordingState::Idle));
    }

    #[test]
    fn test_app_invalid_transition() {
        let mut app = SoundRecorderApp::new();
        assert!(!app.transition_to(RecordingState::Stopped));
        assert_eq!(app.state, RecordingState::Idle);
    }

    #[test]
    fn test_app_process_samples() {
        let mut app = SoundRecorderApp::new();
        app.transition_to(RecordingState::Recording);
        app.process_samples(&[5000, -5000, 3000, -3000]);
        assert!(app.wav.samples.len() > 0);
    }

    #[test]
    fn test_app_process_samples_ignored_when_idle() {
        let mut app = SoundRecorderApp::new();
        app.process_samples(&[5000, -5000]);
        assert_eq!(app.wav.samples.len(), 0);
    }

    #[test]
    fn test_app_set_preset() {
        let mut app = SoundRecorderApp::new();
        app.set_preset(QualityPreset::Voice);
        assert_eq!(app.preset, QualityPreset::Voice);
        assert_eq!(app.sample_rate, SampleRate::Hz8000);
    }

    #[test]
    fn test_app_select_device() {
        let mut app = SoundRecorderApp::new();
        assert!(app.select_device(1));
        assert_eq!(app.selected_device, 1);
        assert!(!app.select_device(999));
    }

    #[test]
    fn test_app_add_marker_during_recording() {
        let mut app = SoundRecorderApp::new();
        app.transition_to(RecordingState::Recording);
        let id = app.add_marker("Test".into());
        assert!(id.is_some());
        assert_eq!(app.markers.len(), 1);
    }

    #[test]
    fn test_app_add_marker_idle_fails() {
        let mut app = SoundRecorderApp::new();
        assert!(app.add_marker("Test".into()).is_none());
    }

    #[test]
    fn test_app_stop_sets_trim() {
        let mut app = SoundRecorderApp::new();
        app.transition_to(RecordingState::Recording);
        app.process_samples(&[1000; 100]);
        app.transition_to(RecordingState::Stopped);
        assert!(app.trim.is_some());
    }

    #[test]
    fn test_app_save_recording() {
        let mut app = SoundRecorderApp::new();
        app.transition_to(RecordingState::Recording);
        app.process_samples(&[1000; 100]);
        app.transition_to(RecordingState::Stopped);
        let id = app.save_recording("test.wav".into());
        assert!(id.is_some());
        assert_eq!(app.history.len(), 1);
    }

    #[test]
    fn test_app_save_recording_idle_fails() {
        let mut app = SoundRecorderApp::new();
        assert!(app.save_recording("test.wav".into()).is_none());
    }

    #[test]
    fn test_app_tick() {
        let mut app = SoundRecorderApp::new();
        app.transition_to(RecordingState::Recording);
        app.tick(1000);
        assert!((app.timer.elapsed_secs() - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_app_tick_idle_no_effect() {
        let mut app = SoundRecorderApp::new();
        app.tick(5000);
        assert_eq!(app.timer.elapsed_secs(), 0.0);
    }

    #[test]
    fn test_app_render_produces_commands() {
        let app = SoundRecorderApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_stopped_has_playback() {
        let mut app = SoundRecorderApp::new();
        app.transition_to(RecordingState::Recording);
        app.process_samples(&[1000; 100]);
        app.transition_to(RecordingState::Stopped);
        let cmds = app.render();
        // Should have more commands due to playback bar + trim handles
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_app_current_device() {
        let app = SoundRecorderApp::new();
        let device = app.current_device().expect("should have default device");
        assert!(device.is_default);
    }

    #[test]
    fn test_app_auto_save_during_recording() {
        let mut app = SoundRecorderApp::new();
        app.auto_save.set_interval_secs(1);
        app.transition_to(RecordingState::Recording);
        assert!(!app.check_auto_save(500));
        assert!(app.check_auto_save(600));
    }

    #[test]
    fn test_app_auto_save_idle_no_trigger() {
        let mut app = SoundRecorderApp::new();
        app.auto_save.set_interval_secs(1);
        assert!(!app.check_auto_save(5000));
    }
}
