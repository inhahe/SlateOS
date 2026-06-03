//! OurOS Sound Mixer
//!
//! A per-app volume mixer similar to Windows Volume Mixer or PulseAudio
//! Volume Control. Shows currently-playing applications with individual
//! volume controls, master volume, device selection, and peak meters.
//!
//! Uses the guitk library for UI rendering with Catppuccin Mocha theme.

#![allow(dead_code)]

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
#[allow(unused_imports)]
use guitk::layout::{FlexAlign, FlexDirection, FlexItem, FlexJustify, FlexLayout, SizeConstraint};
#[allow(unused_imports)]
use guitk::render::RenderTree;
#[allow(unused_imports)]
use guitk::style::{Borders, CornerRadii, Edges, FontWeight, Style, TextAlign};
#[allow(unused_imports)]
use guitk::widget::{Widget, WidgetId, WidgetTree};

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
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    pub const OVERLAY1: Color = Color::from_hex(0x7F849C);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const MAUVE: Color = Color::from_hex(0xCBA6F7);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const TEAL: Color = Color::from_hex(0x94E2D5);
    pub const LAVENDER: Color = Color::from_hex(0xB4BEFE);
}

// ============================================================================
// Audio device types
// ============================================================================

/// An audio device (output or input).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDevice {
    /// Unique device identifier.
    pub id: u32,
    /// Human-readable device name.
    pub name: String,
    /// Device type (output or input).
    pub device_type: DeviceType,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Bit depth (e.g., 16, 24, 32).
    pub bit_depth: u8,
    /// Number of audio channels.
    pub channels: u8,
}

/// Whether a device is for output or input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceType {
    Output,
    Input,
}

// ============================================================================
// Audio stream (per-app)
// ============================================================================

/// Represents a single application's audio stream.
#[derive(Clone, Debug)]
pub struct AudioStream {
    /// Unique stream identifier.
    pub id: u32,
    /// Application name.
    pub app_name: String,
    /// Volume level: 0.0 to 1.0.
    pub volume: f32,
    /// Whether the stream is muted.
    pub muted: bool,
    /// Whether the app is currently producing audio.
    pub playing: bool,
    /// Current peak level: 0.0 to 1.0 (for the peak meter).
    pub peak_level: f32,
    /// Icon identifier (placeholder for now).
    pub icon_id: u32,
}

impl AudioStream {
    /// Create a new audio stream with the given name and volume.
    pub fn new(id: u32, app_name: &str, volume: f32) -> Self {
        Self {
            id,
            app_name: app_name.to_string(),
            volume: volume.clamp(0.0, 1.0),
            muted: false,
            playing: true,
            peak_level: 0.0,
            icon_id: 0,
        }
    }

    /// Get the effective volume (0.0 if muted, otherwise the set volume).
    pub fn effective_volume(&self) -> f32 {
        if self.muted { 0.0 } else { self.volume }
    }

    /// Set volume, clamping to valid range.
    pub fn set_volume(&mut self, vol: f32) {
        self.volume = vol.clamp(0.0, 1.0);
    }

    /// Toggle mute state.
    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
    }

    /// Get volume as a percentage (0-100).
    pub fn volume_percent(&self) -> u8 {
        (self.volume * 100.0).round() as u8
    }
}

// ============================================================================
// Selection state
// ============================================================================

/// Which element is currently selected for keyboard navigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    /// The master volume column.
    Master,
    /// An app stream column (by index in the sorted stream list).
    Stream(usize),
}

impl Selection {
    /// Move selection to the left.
    pub fn move_left(self, stream_count: usize) -> Self {
        match self {
            Self::Master => {
                // Wrap to last stream.
                if stream_count > 0 {
                    Self::Stream(stream_count - 1)
                } else {
                    Self::Master
                }
            }
            Self::Stream(0) => Self::Master,
            Self::Stream(idx) => Self::Stream(idx - 1),
        }
    }

    /// Move selection to the right.
    pub fn move_right(self, stream_count: usize) -> Self {
        match self {
            Self::Master => {
                if stream_count > 0 {
                    Self::Stream(0)
                } else {
                    Self::Master
                }
            }
            Self::Stream(idx) => {
                if idx + 1 < stream_count {
                    Self::Stream(idx + 1)
                } else {
                    // Wrap to master.
                    Self::Master
                }
            }
        }
    }
}

// ============================================================================
// Mixer state
// ============================================================================

/// Top-level mixer application state.
pub struct MixerState {
    /// Master volume: 0.0 to 1.0.
    pub master_volume: f32,
    /// Whether master is muted.
    pub master_muted: bool,
    /// Per-app audio streams.
    pub streams: Vec<AudioStream>,
    /// Available output devices.
    pub output_devices: Vec<AudioDevice>,
    /// Currently selected output device index.
    pub selected_output: usize,
    /// Available input devices.
    pub input_devices: Vec<AudioDevice>,
    /// Currently selected input device index.
    pub selected_input: usize,
    /// Currently focused/selected column.
    pub selection: Selection,
    /// Whether the device dropdown is open.
    pub output_dropdown_open: bool,
    /// Whether the input dropdown is open.
    pub input_dropdown_open: bool,
    /// Simulated tick counter for peak meter animation.
    pub tick_counter: u64,
}

impl MixerState {
    /// Create a new mixer state with stub data.
    pub fn new_with_stubs() -> Self {
        let streams = vec![
            AudioStream {
                id: 1,
                app_name: "Music Player".to_string(),
                volume: 0.80,
                muted: false,
                playing: true,
                peak_level: 0.72,
                icon_id: 1,
            },
            AudioStream {
                id: 2,
                app_name: "Firefox".to_string(),
                volume: 0.65,
                muted: false,
                playing: true,
                peak_level: 0.45,
                icon_id: 2,
            },
            AudioStream {
                id: 3,
                app_name: "System Sounds".to_string(),
                volume: 0.50,
                muted: false,
                playing: false,
                peak_level: 0.0,
                icon_id: 3,
            },
            AudioStream {
                id: 4,
                app_name: "Discord".to_string(),
                volume: 0.90,
                muted: false,
                playing: true,
                peak_level: 0.60,
                icon_id: 4,
            },
            AudioStream {
                id: 5,
                app_name: "Game".to_string(),
                volume: 0.75,
                muted: true,
                playing: false,
                peak_level: 0.0,
                icon_id: 5,
            },
        ];

        let output_devices = vec![
            AudioDevice {
                id: 1,
                name: "Speakers".to_string(),
                device_type: DeviceType::Output,
                sample_rate: 48000,
                bit_depth: 24,
                channels: 2,
            },
            AudioDevice {
                id: 2,
                name: "Headphones".to_string(),
                device_type: DeviceType::Output,
                sample_rate: 96000,
                bit_depth: 32,
                channels: 2,
            },
            AudioDevice {
                id: 3,
                name: "HDMI Output".to_string(),
                device_type: DeviceType::Output,
                sample_rate: 48000,
                bit_depth: 24,
                channels: 8,
            },
        ];

        let input_devices = vec![
            AudioDevice {
                id: 10,
                name: "Microphone".to_string(),
                device_type: DeviceType::Input,
                sample_rate: 48000,
                bit_depth: 16,
                channels: 1,
            },
            AudioDevice {
                id: 11,
                name: "Line In".to_string(),
                device_type: DeviceType::Input,
                sample_rate: 44100,
                bit_depth: 24,
                channels: 2,
            },
        ];

        Self {
            master_volume: 0.75,
            master_muted: false,
            streams,
            output_devices,
            input_devices,
            selected_output: 0,
            selected_input: 0,
            selection: Selection::Master,
            output_dropdown_open: false,
            input_dropdown_open: false,
            tick_counter: 0,
        }
    }

    /// Get master effective volume.
    pub fn master_effective_volume(&self) -> f32 {
        if self.master_muted { 0.0 } else { self.master_volume }
    }

    /// Set master volume, clamped to valid range.
    pub fn set_master_volume(&mut self, vol: f32) {
        self.master_volume = vol.clamp(0.0, 1.0);
    }

    /// Toggle master mute.
    pub fn toggle_master_mute(&mut self) {
        self.master_muted = !self.master_muted;
    }

    /// Get master volume as a percentage (0-100).
    pub fn master_volume_percent(&self) -> u8 {
        (self.master_volume * 100.0).round() as u8
    }

    /// Sort streams: currently playing first, then by app name alphabetically.
    pub fn sorted_streams(&self) -> Vec<&AudioStream> {
        let mut sorted: Vec<&AudioStream> = self.streams.iter().collect();
        sorted.sort_by(|a, b| {
            // Playing streams come first.
            b.playing.cmp(&a.playing)
                .then_with(|| a.app_name.cmp(&b.app_name))
        });
        sorted
    }

    /// Get the currently selected output device.
    pub fn current_output_device(&self) -> Option<&AudioDevice> {
        self.output_devices.get(self.selected_output)
    }

    /// Get the currently selected input device.
    pub fn current_input_device(&self) -> Option<&AudioDevice> {
        self.input_devices.get(self.selected_input)
    }

    /// Adjust volume of the currently selected column.
    pub fn adjust_selected_volume(&mut self, delta: f32) {
        match self.selection {
            Selection::Master => {
                let new_vol = (self.master_volume + delta).clamp(0.0, 1.0);
                self.master_volume = new_vol;
            }
            Selection::Stream(idx) => {
                let sorted_ids: Vec<u32> = self.sorted_streams().iter().map(|s| s.id).collect();
                if let Some(&stream_id) = sorted_ids.get(idx)
                    && let Some(stream) = self.streams.iter_mut().find(|s| s.id == stream_id) {
                        let new_vol = (stream.volume + delta).clamp(0.0, 1.0);
                        stream.volume = new_vol;
                    }
            }
        }
    }

    /// Toggle mute on the currently selected column.
    pub fn toggle_selected_mute(&mut self) {
        match self.selection {
            Selection::Master => {
                self.master_muted = !self.master_muted;
            }
            Selection::Stream(idx) => {
                let sorted_ids: Vec<u32> = self.sorted_streams().iter().map(|s| s.id).collect();
                if let Some(&stream_id) = sorted_ids.get(idx)
                    && let Some(stream) = self.streams.iter_mut().find(|s| s.id == stream_id) {
                        stream.muted = !stream.muted;
                    }
            }
        }
    }

    /// Simulate peak meter updates (called on tick events).
    pub fn update_peak_meters(&mut self) {
        self.tick_counter = self.tick_counter.wrapping_add(1);

        for stream in &mut self.streams {
            if stream.playing && !stream.muted {
                // Simulate fluctuating peak levels using simple pseudo-randomness.
                let seed = stream.id as u64 * 7 + self.tick_counter * 13;
                let pseudo_random = ((seed % 100) as f32) / 100.0;
                // Peak oscillates around the volume level.
                let target = stream.volume * 0.8 * pseudo_random;
                // Smooth towards target (attack fast, decay slow).
                if target > stream.peak_level {
                    stream.peak_level = stream.peak_level + (target - stream.peak_level) * 0.6;
                } else {
                    stream.peak_level = stream.peak_level + (target - stream.peak_level) * 0.15;
                }
                stream.peak_level = stream.peak_level.clamp(0.0, 1.0);
            } else {
                // Decay to zero when not playing or muted.
                stream.peak_level *= 0.85;
                if stream.peak_level < 0.01 {
                    stream.peak_level = 0.0;
                }
            }
        }
    }
}

// ============================================================================
// Volume calculations
// ============================================================================

/// Convert a linear volume (0.0 - 1.0) to decibels.
/// Returns -infinity for 0.0.
pub fn linear_to_db(linear: f32) -> f32 {
    if linear <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * linear.log10()
    }
}

/// Convert decibels to linear volume (0.0 - 1.0).
/// Clamps at 0.0 for very low dB values.
pub fn db_to_linear(db: f32) -> f32 {
    if db <= -80.0 {
        0.0
    } else {
        let linear = 10.0_f32.powf(db / 20.0);
        linear.clamp(0.0, 1.0)
    }
}

/// Format volume as a percentage string (e.g., "75%").
pub fn format_volume_percent(volume: f32) -> String {
    let pct = (volume.clamp(0.0, 1.0) * 100.0).round() as u8;
    format!("{}%", pct)
}

/// Format volume in decibels (e.g., "-6.0 dB").
pub fn format_volume_db(volume: f32) -> String {
    let db = linear_to_db(volume);
    if db == f32::NEG_INFINITY {
        "-inf dB".to_string()
    } else {
        format!("{:.1} dB", db)
    }
}

/// Calculate the combined volume for an app (app volume * master volume).
pub fn combined_volume(app_volume: f32, master_volume: f32) -> f32 {
    (app_volume * master_volume).clamp(0.0, 1.0)
}

// ============================================================================
// UI rendering
// ============================================================================

/// Build the full mixer widget tree.
pub fn build_mixer_ui(state: &MixerState) -> Widget {
    let mut root = Widget::container()
        .with_background(colors::BASE)
        .with_flex_direction(FlexDirection::Column)
        .with_style(Style {
            background: colors::BASE,
            padding: Edges::all(12.0),
            min_width: Some(600.0),
            min_height: Some(400.0),
            ..Style::default()
        });

    // Device selection bar at top.
    let device_bar = build_device_bar(state);
    root.children.push(device_bar);

    // Separator.
    root.children.push(build_separator());

    // Main content: master + app columns in a horizontal scroll area.
    let mut content_row = Widget::container()
        .with_flex_direction(FlexDirection::Row)
        .with_flex_grow(1.0)
        .with_style(Style {
            padding: Edges::symmetric(8.0, 0.0),
            ..Style::default()
        });

    // Flex layout with gap between columns.
    content_row.flex_layout = Some(FlexLayout {
        direction: FlexDirection::Row,
        gap: 16.0,
        align_items: FlexAlign::Stretch,
        ..FlexLayout::default()
    });

    // Master volume column (larger).
    content_row.children.push(build_master_column(state));

    // Vertical separator between master and apps.
    content_row.children.push(build_vertical_separator());

    // Per-app columns (sorted: playing first, then by name).
    let sorted = state.sorted_streams();
    for (idx, stream) in sorted.iter().enumerate() {
        let selected = state.selection == Selection::Stream(idx);
        content_row.children.push(build_stream_column(stream, selected));
    }

    root.children.push(content_row);

    // Keyboard shortcut hint at bottom.
    root.children.push(build_shortcut_bar());

    root
}

/// Build the device selection bar.
fn build_device_bar(state: &MixerState) -> Widget {
    let mut bar = Widget::container()
        .with_flex_direction(FlexDirection::Row)
        .with_style(Style {
            background: colors::MANTLE,
            padding: Edges::symmetric(8.0, 12.0),
            border_radius: CornerRadii::all(6.0),
            margin: Edges { top: 0.0, right: 0.0, bottom: 8.0, left: 0.0 },
            ..Style::default()
        });

    bar.flex_layout = Some(FlexLayout {
        direction: FlexDirection::Row,
        gap: 24.0,
        align_items: FlexAlign::Center,
        justify: FlexJustify::SpaceBetween,
        ..FlexLayout::default()
    });

    // Output device section.
    let mut output_section = Widget::container()
        .with_flex_direction(FlexDirection::Row);
    output_section.flex_layout = Some(FlexLayout {
        direction: FlexDirection::Row,
        gap: 8.0,
        align_items: FlexAlign::Center,
        ..FlexLayout::default()
    });

    output_section.children.push(
        Widget::label("Output:")
            .with_style(Style {
                foreground: colors::SUBTEXT0,
                font_weight: FontWeight::Bold,
                ..Style::default()
            })
    );

    let output_name = state.current_output_device()
        .map(|d| d.name.as_str())
        .unwrap_or("None");
    output_section.children.push(
        Widget::label(output_name)
            .with_style(Style {
                foreground: colors::BLUE,
                ..Style::default()
            })
    );

    // Output device properties.
    if let Some(dev) = state.current_output_device() {
        let props = format!("{}Hz / {}bit / {}ch", dev.sample_rate, dev.bit_depth, dev.channels);
        output_section.children.push(
            Widget::label(&props)
                .with_style(Style {
                    foreground: colors::OVERLAY1,
                    font_size: 11.0,
                    ..Style::default()
                })
        );
    }

    bar.children.push(output_section);

    // Input device section.
    let mut input_section = Widget::container()
        .with_flex_direction(FlexDirection::Row);
    input_section.flex_layout = Some(FlexLayout {
        direction: FlexDirection::Row,
        gap: 8.0,
        align_items: FlexAlign::Center,
        ..FlexLayout::default()
    });

    input_section.children.push(
        Widget::label("Input:")
            .with_style(Style {
                foreground: colors::SUBTEXT0,
                font_weight: FontWeight::Bold,
                ..Style::default()
            })
    );

    let input_name = state.current_input_device()
        .map(|d| d.name.as_str())
        .unwrap_or("None");
    input_section.children.push(
        Widget::label(input_name)
            .with_style(Style {
                foreground: colors::TEAL,
                ..Style::default()
            })
    );

    if let Some(dev) = state.current_input_device() {
        let props = format!("{}Hz / {}bit / {}ch", dev.sample_rate, dev.bit_depth, dev.channels);
        input_section.children.push(
            Widget::label(&props)
                .with_style(Style {
                    foreground: colors::OVERLAY1,
                    font_size: 11.0,
                    ..Style::default()
                })
        );
    }

    bar.children.push(input_section);

    bar
}

/// Build the master volume column.
fn build_master_column(state: &MixerState) -> Widget {
    let selected = state.selection == Selection::Master;
    let border_color = if selected { colors::MAUVE } else { colors::SURFACE1 };

    let mut col = Widget::container()
        .with_flex_direction(FlexDirection::Column)
        .with_style(Style {
            background: colors::SURFACE0,
            padding: Edges::all(12.0),
            border: Borders::all(2.0, border_color),
            border_radius: CornerRadii::all(8.0),
            min_width: Some(90.0),
            ..Style::default()
        });

    col.flex_layout = Some(FlexLayout {
        direction: FlexDirection::Column,
        gap: 8.0,
        align_items: FlexAlign::Center,
        ..FlexLayout::default()
    });

    // Title.
    col.children.push(
        Widget::label("Master")
            .with_style(Style {
                foreground: colors::TEXT,
                font_weight: FontWeight::Bold,
                font_size: 13.0,
                ..Style::default()
            })
    );

    // Volume percentage display.
    let vol_text = format_volume_percent(state.master_volume);
    col.children.push(
        Widget::label(&vol_text)
            .with_style(Style {
                foreground: if state.master_muted { colors::RED } else { colors::GREEN },
                font_weight: FontWeight::Bold,
                font_size: 16.0,
                ..Style::default()
            })
    );

    // Volume slider (vertical representation via progress bar).
    let slider_value = if state.master_muted { 0.0 } else { state.master_volume };
    col.children.push(build_volume_slider(slider_value, true));

    // Mute button.
    let mute_text = if state.master_muted { "MUTED" } else { "Mute" };
    let mute_bg = if state.master_muted { colors::RED } else { colors::SURFACE1 };
    col.children.push(
        Widget::button(mute_text)
            .with_style(Style {
                background: mute_bg,
                foreground: colors::TEXT,
                padding: Edges::symmetric(4.0, 12.0),
                border_radius: CornerRadii::all(4.0),
                ..Style::default()
            })
    );

    // dB display.
    let db_text = format_volume_db(state.master_effective_volume());
    col.children.push(
        Widget::label(&db_text)
            .with_style(Style {
                foreground: colors::OVERLAY1,
                font_size: 10.0,
                ..Style::default()
            })
    );

    col
}

/// Build a per-app stream column.
fn build_stream_column(stream: &AudioStream, selected: bool) -> Widget {
    let border_color = if selected { colors::MAUVE } else { colors::SURFACE1 };
    let bg = if stream.playing { colors::SURFACE0 } else { colors::MANTLE };

    let mut col = Widget::container()
        .with_flex_direction(FlexDirection::Column)
        .with_style(Style {
            background: bg,
            padding: Edges::all(10.0),
            border: Borders::all(if selected { 2.0 } else { 1.0 }, border_color),
            border_radius: CornerRadii::all(8.0),
            min_width: Some(80.0),
            ..Style::default()
        });

    col.flex_layout = Some(FlexLayout {
        direction: FlexDirection::Column,
        gap: 6.0,
        align_items: FlexAlign::Center,
        ..FlexLayout::default()
    });

    // App name.
    let name_color = if stream.playing { colors::TEXT } else { colors::OVERLAY1 };
    col.children.push(
        Widget::label(&stream.app_name)
            .with_style(Style {
                foreground: name_color,
                font_weight: FontWeight::Bold,
                font_size: 11.0,
                ..Style::default()
            })
    );

    // Playing indicator.
    let status_text = if stream.playing { "Playing" } else { "Idle" };
    let status_color = if stream.playing { colors::GREEN } else { colors::OVERLAY0 };
    col.children.push(
        Widget::label(status_text)
            .with_style(Style {
                foreground: status_color,
                font_size: 9.0,
                ..Style::default()
            })
    );

    // Volume percentage.
    let vol_text = format_volume_percent(stream.volume);
    col.children.push(
        Widget::label(&vol_text)
            .with_style(Style {
                foreground: if stream.muted { colors::RED } else { colors::SUBTEXT1 },
                font_size: 12.0,
                ..Style::default()
            })
    );

    // Volume slider.
    let slider_value = if stream.muted { 0.0 } else { stream.volume };
    col.children.push(build_volume_slider(slider_value, false));

    // Peak meter.
    col.children.push(build_peak_meter(stream.peak_level));

    // Mute button.
    let mute_text = if stream.muted { "M" } else { "m" };
    let mute_bg = if stream.muted { colors::RED } else { colors::SURFACE1 };
    col.children.push(
        Widget::button(mute_text)
            .with_style(Style {
                background: mute_bg,
                foreground: colors::TEXT,
                padding: Edges::symmetric(3.0, 8.0),
                border_radius: CornerRadii::all(4.0),
                ..Style::default()
            })
    );

    col
}

/// Build a volume slider widget (represented as a progress bar).
fn build_volume_slider(value: f32, is_master: bool) -> Widget {
    let height = if is_master { 150.0 } else { 120.0 };
    let width = if is_master { 24.0 } else { 18.0 };

    // Slider track (vertical progress bar representation).
    let track_color = colors::SURFACE1;
    let fill_color = if value > 0.8 {
        colors::YELLOW
    } else if value > 0.0 {
        colors::GREEN
    } else {
        colors::OVERLAY0
    };

    let mut slider = Widget::container()
        .with_style(Style {
            background: track_color,
            border_radius: CornerRadii::all(4.0),
            min_width: Some(width),
            min_height: Some(height),
            ..Style::default()
        });

    // Fill portion.
    let fill_height = height * value;
    slider.children.push(
        Widget::container()
            .with_style(Style {
                background: fill_color,
                border_radius: CornerRadii::all(3.0),
                min_width: Some(width - 4.0),
                min_height: Some(fill_height),
                margin: Edges { top: height - fill_height, right: 2.0, bottom: 0.0, left: 2.0 },
                ..Style::default()
            })
    );

    slider
}

/// Build a peak level meter.
fn build_peak_meter(level: f32) -> Widget {
    let meter_height = 100.0;
    let meter_width = 8.0;

    // Choose color based on level.
    let fill_color = if level > 0.9 {
        colors::RED
    } else if level > 0.7 {
        colors::YELLOW
    } else if level > 0.0 {
        colors::GREEN
    } else {
        colors::SURFACE1
    };

    let mut meter = Widget::container()
        .with_style(Style {
            background: colors::CRUST,
            border_radius: CornerRadii::all(3.0),
            border: Borders::all(1.0, colors::SURFACE1),
            min_width: Some(meter_width),
            min_height: Some(meter_height),
            ..Style::default()
        });

    let fill_height = meter_height * level;
    meter.children.push(
        Widget::container()
            .with_style(Style {
                background: fill_color,
                border_radius: CornerRadii::all(2.0),
                min_width: Some(meter_width - 4.0),
                min_height: Some(fill_height),
                margin: Edges { top: meter_height - fill_height, right: 2.0, bottom: 0.0, left: 2.0 },
                ..Style::default()
            })
    );

    meter
}

/// Build a horizontal separator.
fn build_separator() -> Widget {
    Widget::container()
        .with_style(Style {
            background: colors::SURFACE1,
            min_height: Some(1.0),
            margin: Edges::symmetric(8.0, 0.0),
            ..Style::default()
        })
        .with_flex_grow(0.0)
}

/// Build a vertical separator.
fn build_vertical_separator() -> Widget {
    Widget::container()
        .with_style(Style {
            background: colors::SURFACE1,
            min_width: Some(1.0),
            margin: Edges::symmetric(0.0, 4.0),
            ..Style::default()
        })
}

/// Build the keyboard shortcut hint bar at the bottom.
fn build_shortcut_bar() -> Widget {
    let mut bar = Widget::container()
        .with_flex_direction(FlexDirection::Row)
        .with_style(Style {
            background: colors::MANTLE,
            padding: Edges::symmetric(6.0, 12.0),
            border_radius: CornerRadii::all(4.0),
            margin: Edges { top: 8.0, right: 0.0, bottom: 0.0, left: 0.0 },
            ..Style::default()
        });

    bar.flex_layout = Some(FlexLayout {
        direction: FlexDirection::Row,
        gap: 16.0,
        align_items: FlexAlign::Center,
        justify: FlexJustify::Center,
        ..FlexLayout::default()
    });

    let shortcuts = [
        ("Left/Right", "Select"),
        ("Up/Down", "Volume"),
        ("M", "Mute"),
        ("Tab", "Cycle"),
    ];

    for (key, action) in &shortcuts {
        let mut item = Widget::container()
            .with_flex_direction(FlexDirection::Row);
        item.flex_layout = Some(FlexLayout {
            direction: FlexDirection::Row,
            gap: 4.0,
            align_items: FlexAlign::Center,
            ..FlexLayout::default()
        });

        item.children.push(
            Widget::label(key)
                .with_style(Style {
                    foreground: colors::LAVENDER,
                    font_weight: FontWeight::Bold,
                    font_size: 10.0,
                    ..Style::default()
                })
        );
        item.children.push(
            Widget::label(action)
                .with_style(Style {
                    foreground: colors::OVERLAY1,
                    font_size: 10.0,
                    ..Style::default()
                })
        );

        bar.children.push(item);
    }

    bar
}

// ============================================================================
// Event handling
// ============================================================================

/// Volume adjustment step size per key press.
const VOLUME_STEP: f32 = 0.05;

/// Handle keyboard events for the mixer.
pub fn handle_key_event(state: &mut MixerState, event: &KeyEvent) -> EventResult {
    if !event.pressed {
        return EventResult::Ignored;
    }

    match event.key {
        Key::Left => {
            let stream_count = state.streams.iter().filter(|s| s.playing).count()
                + state.streams.iter().filter(|s| !s.playing).count();
            state.selection = state.selection.move_left(stream_count);
            EventResult::Consumed
        }
        Key::Right => {
            let stream_count = state.streams.len();
            state.selection = state.selection.move_right(stream_count);
            EventResult::Consumed
        }
        Key::Up => {
            state.adjust_selected_volume(VOLUME_STEP);
            EventResult::Consumed
        }
        Key::Down => {
            state.adjust_selected_volume(-VOLUME_STEP);
            EventResult::Consumed
        }
        Key::M => {
            state.toggle_selected_mute();
            EventResult::Consumed
        }
        Key::Tab => {
            // Cycle: Master -> Stream(0) -> Stream(1) -> ... -> Master.
            let stream_count = state.streams.len();
            state.selection = state.selection.move_right(stream_count);
            EventResult::Consumed
        }
        Key::Escape => {
            // Could close the app in a real implementation.
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

/// Handle mouse click on a slider (simplified: assumes click anywhere on
/// a stream column adjusts that stream's volume proportionally).
pub fn handle_slider_click(state: &mut MixerState, column_index: Option<usize>, y_fraction: f32) {
    let volume = (1.0 - y_fraction).clamp(0.0, 1.0);

    match column_index {
        None => {
            // Master column.
            state.master_volume = volume;
            state.selection = Selection::Master;
        }
        Some(idx) => {
            let sorted_ids: Vec<u32> = state.sorted_streams().iter().map(|s| s.id).collect();
            if let Some(&stream_id) = sorted_ids.get(idx)
                && let Some(stream) = state.streams.iter_mut().find(|s| s.id == stream_id) {
                    stream.volume = volume;
                }
            state.selection = Selection::Stream(idx);
        }
    }
}

// ============================================================================
// Application entry point
// ============================================================================

fn main() {
    let mut state = MixerState::new_with_stubs();

    // Build initial UI.
    let _ui = build_mixer_ui(&state);

    // Simulate a few tick updates for peak meters.
    for _ in 0..10 {
        state.update_peak_meters();
    }

    // In a real implementation, this would enter the event loop provided
    // by the windowing system / compositor. For now, we just build the UI
    // and verify it constructs correctly.
    let _final_ui = build_mixer_ui(&state);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // === Volume calculation tests ===

    #[test]
    fn test_linear_to_db_zero() {
        assert_eq!(linear_to_db(0.0), f32::NEG_INFINITY);
    }

    #[test]
    fn test_linear_to_db_unity() {
        let db = linear_to_db(1.0);
        assert!((db - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_linear_to_db_half() {
        let db = linear_to_db(0.5);
        // -6.02 dB
        assert!((db - (-6.0206)).abs() < 0.01);
    }

    #[test]
    fn test_db_to_linear_zero_db() {
        let linear = db_to_linear(0.0);
        assert!((linear - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_db_to_linear_minus_6() {
        let linear = db_to_linear(-6.0206);
        assert!((linear - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_db_to_linear_very_low() {
        assert_eq!(db_to_linear(-80.0), 0.0);
        assert_eq!(db_to_linear(-100.0), 0.0);
    }

    #[test]
    fn test_db_roundtrip() {
        let values = [0.1, 0.25, 0.5, 0.75, 1.0];
        for &v in &values {
            let db = linear_to_db(v);
            let back = db_to_linear(db);
            assert!((back - v).abs() < 0.001, "roundtrip failed for {}: got {}", v, back);
        }
    }

    #[test]
    fn test_combined_volume() {
        assert!((combined_volume(0.5, 0.5) - 0.25).abs() < 0.001);
        assert!((combined_volume(1.0, 1.0) - 1.0).abs() < 0.001);
        assert!((combined_volume(0.0, 1.0) - 0.0).abs() < 0.001);
        assert!((combined_volume(1.0, 0.0) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_combined_volume_clamps() {
        // Even with values > 1.0 due to floating point, result is clamped.
        assert!(combined_volume(1.5, 1.5) <= 1.0);
    }

    #[test]
    fn test_format_volume_percent() {
        assert_eq!(format_volume_percent(0.0), "0%");
        assert_eq!(format_volume_percent(0.75), "75%");
        assert_eq!(format_volume_percent(1.0), "100%");
        assert_eq!(format_volume_percent(0.333), "33%");
    }

    #[test]
    fn test_format_volume_db() {
        assert_eq!(format_volume_db(0.0), "-inf dB");
        assert_eq!(format_volume_db(1.0), "0.0 dB");
    }

    // === Mute logic tests ===

    #[test]
    fn test_stream_effective_volume_normal() {
        let stream = AudioStream::new(1, "Test", 0.75);
        assert!((stream.effective_volume() - 0.75).abs() < 0.001);
    }

    #[test]
    fn test_stream_effective_volume_muted() {
        let mut stream = AudioStream::new(1, "Test", 0.75);
        stream.muted = true;
        assert_eq!(stream.effective_volume(), 0.0);
    }

    #[test]
    fn test_stream_toggle_mute() {
        let mut stream = AudioStream::new(1, "Test", 0.50);
        assert!(!stream.muted);
        stream.toggle_mute();
        assert!(stream.muted);
        assert_eq!(stream.effective_volume(), 0.0);
        stream.toggle_mute();
        assert!(!stream.muted);
        assert!((stream.effective_volume() - 0.50).abs() < 0.001);
    }

    #[test]
    fn test_master_effective_volume() {
        let mut state = MixerState::new_with_stubs();
        state.master_volume = 0.60;
        state.master_muted = false;
        assert!((state.master_effective_volume() - 0.60).abs() < 0.001);

        state.master_muted = true;
        assert_eq!(state.master_effective_volume(), 0.0);
    }

    #[test]
    fn test_master_toggle_mute() {
        let mut state = MixerState::new_with_stubs();
        assert!(!state.master_muted);
        state.toggle_master_mute();
        assert!(state.master_muted);
        state.toggle_master_mute();
        assert!(!state.master_muted);
    }

    // === Volume clamping tests ===

    #[test]
    fn test_set_volume_clamps_high() {
        let mut stream = AudioStream::new(1, "Test", 0.5);
        stream.set_volume(1.5);
        assert_eq!(stream.volume, 1.0);
    }

    #[test]
    fn test_set_volume_clamps_low() {
        let mut stream = AudioStream::new(1, "Test", 0.5);
        stream.set_volume(-0.5);
        assert_eq!(stream.volume, 0.0);
    }

    #[test]
    fn test_master_set_volume_clamps() {
        let mut state = MixerState::new_with_stubs();
        state.set_master_volume(2.0);
        assert_eq!(state.master_volume, 1.0);
        state.set_master_volume(-1.0);
        assert_eq!(state.master_volume, 0.0);
    }

    #[test]
    fn test_volume_percent() {
        let stream = AudioStream::new(1, "Test", 0.75);
        assert_eq!(stream.volume_percent(), 75);
    }

    #[test]
    fn test_master_volume_percent() {
        let mut state = MixerState::new_with_stubs();
        state.master_volume = 0.42;
        assert_eq!(state.master_volume_percent(), 42);
    }

    // === Sorting tests ===

    #[test]
    fn test_sorted_streams_playing_first() {
        let state = MixerState::new_with_stubs();
        let sorted = state.sorted_streams();

        // All playing streams should come before non-playing ones.
        let mut seen_non_playing = false;
        for stream in &sorted {
            if !stream.playing {
                seen_non_playing = true;
            } else if seen_non_playing {
                panic!("Playing stream found after non-playing stream in sorted order");
            }
        }
    }

    #[test]
    fn test_sorted_streams_alphabetical_within_group() {
        let mut state = MixerState::new_with_stubs();
        // Set all to playing for consistent sorting.
        for stream in &mut state.streams {
            stream.playing = true;
        }

        let sorted = state.sorted_streams();
        for window in sorted.windows(2) {
            assert!(
                window[0].app_name <= window[1].app_name,
                "Streams not alphabetically sorted: {} > {}",
                window[0].app_name,
                window[1].app_name
            );
        }
    }

    #[test]
    fn test_sorted_streams_non_playing_alphabetical() {
        let mut state = MixerState::new_with_stubs();
        // Set all to not playing.
        for stream in &mut state.streams {
            stream.playing = false;
        }

        let sorted = state.sorted_streams();
        for window in sorted.windows(2) {
            assert!(
                window[0].app_name <= window[1].app_name,
                "Non-playing streams not alphabetically sorted: {} > {}",
                window[0].app_name,
                window[1].app_name
            );
        }
    }

    // === Selection/navigation tests ===

    #[test]
    fn test_selection_move_right_from_master() {
        let sel = Selection::Master;
        assert_eq!(sel.move_right(3), Selection::Stream(0));
    }

    #[test]
    fn test_selection_move_right_wraps() {
        let sel = Selection::Stream(2);
        assert_eq!(sel.move_right(3), Selection::Master);
    }

    #[test]
    fn test_selection_move_left_from_master() {
        let sel = Selection::Master;
        assert_eq!(sel.move_left(3), Selection::Stream(2));
    }

    #[test]
    fn test_selection_move_left_to_master() {
        let sel = Selection::Stream(0);
        assert_eq!(sel.move_left(5), Selection::Master);
    }

    #[test]
    fn test_selection_move_right_no_streams() {
        let sel = Selection::Master;
        assert_eq!(sel.move_right(0), Selection::Master);
    }

    #[test]
    fn test_selection_move_left_no_streams() {
        let sel = Selection::Master;
        assert_eq!(sel.move_left(0), Selection::Master);
    }

    // === Volume adjustment via selection tests ===

    #[test]
    fn test_adjust_master_volume_up() {
        let mut state = MixerState::new_with_stubs();
        state.master_volume = 0.50;
        state.selection = Selection::Master;
        state.adjust_selected_volume(0.05);
        assert!((state.master_volume - 0.55).abs() < 0.001);
    }

    #[test]
    fn test_adjust_master_volume_clamps_at_max() {
        let mut state = MixerState::new_with_stubs();
        state.master_volume = 0.98;
        state.selection = Selection::Master;
        state.adjust_selected_volume(0.05);
        assert_eq!(state.master_volume, 1.0);
    }

    #[test]
    fn test_adjust_stream_volume() {
        let mut state = MixerState::new_with_stubs();
        state.selection = Selection::Stream(0);
        let sorted_ids: Vec<u32> = state.sorted_streams().iter().map(|s| s.id).collect();
        let first_id = sorted_ids[0];
        let initial_vol = state.streams.iter().find(|s| s.id == first_id).unwrap().volume;

        state.adjust_selected_volume(0.05);

        let new_vol = state.streams.iter().find(|s| s.id == first_id).unwrap().volume;
        assert!((new_vol - (initial_vol + 0.05)).abs() < 0.001);
    }

    // === Peak meter tests ===

    #[test]
    fn test_peak_meters_decay_when_not_playing() {
        let mut state = MixerState::new_with_stubs();
        // Set all to not playing with non-zero peaks.
        for stream in &mut state.streams {
            stream.playing = false;
            stream.peak_level = 0.5;
        }

        // After several ticks, peaks should decay toward zero.
        for _ in 0..50 {
            state.update_peak_meters();
        }

        for stream in &state.streams {
            assert!(
                stream.peak_level < 0.01,
                "Peak level should decay to near zero when not playing, got {}",
                stream.peak_level
            );
        }
    }

    #[test]
    fn test_peak_meters_stay_zero_when_muted() {
        let mut state = MixerState::new_with_stubs();
        for stream in &mut state.streams {
            stream.muted = true;
            stream.peak_level = 0.5;
        }

        for _ in 0..50 {
            state.update_peak_meters();
        }

        for stream in &state.streams {
            assert!(
                stream.peak_level < 0.01,
                "Peak level should decay when muted, got {}",
                stream.peak_level
            );
        }
    }

    #[test]
    fn test_peak_meters_bounded() {
        let mut state = MixerState::new_with_stubs();
        for _ in 0..1000 {
            state.update_peak_meters();
        }

        for stream in &state.streams {
            assert!(stream.peak_level >= 0.0 && stream.peak_level <= 1.0);
        }
    }

    // === Slider click handling tests ===

    #[test]
    fn test_slider_click_master() {
        let mut state = MixerState::new_with_stubs();
        handle_slider_click(&mut state, None, 0.25);
        assert!((state.master_volume - 0.75).abs() < 0.001);
        assert_eq!(state.selection, Selection::Master);
    }

    #[test]
    fn test_slider_click_stream() {
        let mut state = MixerState::new_with_stubs();
        handle_slider_click(&mut state, Some(0), 0.5);
        assert_eq!(state.selection, Selection::Stream(0));
    }

    // === Keyboard event handling tests ===

    #[test]
    fn test_handle_key_left() {
        let mut state = MixerState::new_with_stubs();
        state.selection = Selection::Stream(1);
        let event = KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        let result = handle_key_event(&mut state, &event);
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.selection, Selection::Stream(0));
    }

    #[test]
    fn test_handle_key_mute() {
        let mut state = MixerState::new_with_stubs();
        state.selection = Selection::Master;
        state.master_muted = false;
        let event = KeyEvent {
            key: Key::M,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        let result = handle_key_event(&mut state, &event);
        assert_eq!(result, EventResult::Consumed);
        assert!(state.master_muted);
    }

    #[test]
    fn test_handle_key_up_volume() {
        let mut state = MixerState::new_with_stubs();
        state.selection = Selection::Master;
        state.master_volume = 0.50;
        let event = KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        let result = handle_key_event(&mut state, &event);
        assert_eq!(result, EventResult::Consumed);
        assert!((state.master_volume - 0.55).abs() < 0.001);
    }

    #[test]
    fn test_handle_key_release_ignored() {
        let mut state = MixerState::new_with_stubs();
        let event = KeyEvent {
            key: Key::M,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        };
        let result = handle_key_event(&mut state, &event);
        assert_eq!(result, EventResult::Ignored);
    }

    #[test]
    fn test_handle_unknown_key_ignored() {
        let mut state = MixerState::new_with_stubs();
        let event = KeyEvent {
            key: Key::F12,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        let result = handle_key_event(&mut state, &event);
        assert_eq!(result, EventResult::Ignored);
    }

    // === UI building tests ===

    #[test]
    fn test_build_mixer_ui_does_not_panic() {
        let state = MixerState::new_with_stubs();
        let _ui = build_mixer_ui(&state);
    }

    #[test]
    fn test_build_mixer_ui_has_children() {
        let state = MixerState::new_with_stubs();
        let ui = build_mixer_ui(&state);
        // Root should have: device bar, separator, content row, shortcut bar.
        assert_eq!(ui.children.len(), 4);
    }

    #[test]
    fn test_device_bar_shows_correct_device() {
        let state = MixerState::new_with_stubs();
        let ui = build_mixer_ui(&state);
        // The device bar is the first child.
        let device_bar = &ui.children[0];
        // It should have at least 2 children (output section, input section).
        assert!(device_bar.children.len() >= 2);
    }
}
