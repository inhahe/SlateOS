//! Slate OS Sound Mixer, in a real window.
//!
//! A per-app volume mixer: a master column and one column per playing
//! application, each with a draggable vertical fader, a peak meter, a mute
//! button and a live percentage, plus an output and an input device picker.
//!
//! # What wiring this up found
//!
//! `main` built a widget tree, dropped it, ticked the peak meters ten times,
//! built the tree a second time and dropped that too. Nothing below it had
//! ever been reached, so nothing below it had ever been run.
//!
//! 1. **The mixer could not be operated with a mouse.** There was no
//!    `Event::Mouse` arm anywhere in the file — not an unhandled one, not a
//!    stubbed one, none. A volume mixer is a bank of faders; a fader you cannot
//!    drag or click is a picture of a fader.
//! 2. **`handle_slider_click` was a handler with no caller and no way to obtain
//!    its own arguments.** It is `pub`, it takes `column_index: Option<usize>`
//!    and `y_fraction: f32`, and *nothing in the program computed either one*.
//!    There was no code that turned a pixel into a column, because there were no
//!    column rectangles to turn it into: the layout was a `FlexLayout` tree that
//!    the program never measured. Its two tests called it with hand-written
//!    numbers, which is the campaign's recurring shape — a test that hands the
//!    program the answer and checks it uses it.
//! 3. **The peak meters decayed per *call*, not per unit of time.** `update_peak_meters`
//!    took no arguments: attack `0.6`, decay `0.15`, silence `0.85`, once per
//!    invocation. A meter driven from a frame callback therefore falls twice as
//!    fast on a 120Hz screen as on a 60Hz one, and a window that stalls holds its
//!    meters up for as long as the stall lasts. It now advances in fixed 40ms
//!    steps off the `elapsed_ms` a `Tick` carries, with catch-up capped.
//!    `known-issues.md` lesson 47's ninth application.
//! 4. **The two dropdown flags were phantom state — written by nothing and read
//!    by nothing.** `output_dropdown_open` and `input_dropdown_open` sat on the
//!    model with doc comments describing a device picker that did not exist:
//!    `build_device_bar` never consulted them, no key set them, no click could.
//!    `selected_output` and `selected_input` were never written either, so the
//!    device bar named a device that could not be changed. This is life's
//!    phantom viewport again, one step worse — that one was at least read.
//! 5. **`Tab` was a byte-for-byte duplicate of `Right`.** The shortcut bar named
//!    `Left/Right` "Select" and `Tab` "Cycle" as though they were two different
//!    things; they ran the identical three lines. And `Shift-Tab` — the one
//!    keystroke every toolkit on earth agrees means "focus backwards" — did
//!    nothing at all, because no handler in the file looked at a modifier.
//! 6. **`Escape` consumed a keystroke to do nothing**, over the comment "Could
//!    close the app in a real implementation". A key that is swallowed and
//!    discarded is worse than one that is ignored: ignoring it lets the window
//!    manager act on it, swallowing it does not.
//! 7. **No handler filtered modifiers**, so `Ctrl-M` muted and `Ctrl-Left`
//!    moved the selection — a program cannot offer `Ctrl` shortcuts later if it
//!    already answers `Ctrl` as though it were nothing.
//! 8. **`Key::Left` computed the stream count as
//!    `filter(playing).count() + filter(!playing).count()`** — that is `len()`,
//!    written as the sum of a partition. It is harmless, and it is proof that
//!    the line had never been read by anybody, which is the point.
//! 9. **`Selection::move_left` and `move_right` disagreed about what wrapping
//!    means.** `move_right` from the last stream fell through to
//!    `Self::Stream(idx)` — *itself* — so the selection stuck at the right-hand
//!    end, while `move_left` from `Master` wrapped round to the last stream.
//!    One direction was a cycle and the other was a wall.
//! 10. **The window was never sized and never resized.** Every dimension in the
//!     file was a constant inside a `Style`, so there was no layout to read a
//!     click against even if a click had arrived.
//! 11. **`#![allow(dead_code)]` and eight `#[allow(unused_imports)]`** covered
//!     all of the above.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::rng::{RandomSource, SeededRng, seeded_from_system};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

// ── Catppuccin Mocha palette ───────────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E_1E2E);
const MANTLE: Color = Color::from_hex(0x18_1825);
const CRUST: Color = Color::from_hex(0x11_111B);
const SURFACE0: Color = Color::from_hex(0x31_3244);
const SURFACE1: Color = Color::from_hex(0x45_475A);
const OVERLAY1: Color = Color::from_hex(0x7F_849C);
const TEXT_COLOR: Color = Color::from_hex(0xCD_D6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6_ADC8);
const BLUE: Color = Color::from_hex(0x89_B4FA);
const GREEN: Color = Color::from_hex(0xA6_E3A1);
const RED: Color = Color::from_hex(0xF3_8BA8);
const YELLOW: Color = Color::from_hex(0xF9_E2AF);
const TEAL: Color = Color::from_hex(0x94_E2D5);
const LAVENDER: Color = Color::from_hex(0xB4_BEFE);

const WINDOW_WIDTH: f32 = 960.0;
const WINDOW_HEIGHT: f32 = 620.0;

/// The seed the simulated meters fall back to when the kernel has no entropy.
///
/// A peak meter reading is novelty randomness, not a secret, so losing entropy
/// must not stop the mixer from drawing. The constant is per-crate ("MIXER!!!")
/// so two programs falling back on the same boot do not then agree with each
/// other.
const FALLBACK_SEED: u64 = 0x4D49_5845_5221_2121;

/// How much one press of Up or Down moves a fader.
const VOLUME_STEP: f32 = 0.05;

/// The meters advance one step per this many milliseconds of real time, no
/// matter how often the window happens to draw.
///
/// The old code ran one step per call to `update_peak_meters`, which ties the
/// ballistics of the meter to the frame rate: the same audio reads differently
/// on a 60Hz screen and a 120Hz one, and a stalled window freezes its meters
/// rather than letting them fall.
const METER_STEP_MS: u64 = 40;

/// The most meter steps one tick may run before the rest of the backlog is
/// dropped.
///
/// A window suspended for ten seconds comes back owing 250 steps. Running them
/// all in the frame it wakes up in stalls it again, and nobody can see 250
/// steps of a meter anyway — the only visible result is the last one.
const MAX_CATCHUP: u32 = 6;

// ── Devices and streams ────────────────────────────────────────────────────

/// Whether a device carries sound out of the machine or into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceType {
    Output,
    Input,
}

/// An audio device the mixer can be pointed at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDevice {
    pub id: u32,
    pub name: String,
    pub device_type: DeviceType,
    pub sample_rate: u32,
    pub bit_depth: u8,
    pub channels: u8,
}

impl AudioDevice {
    /// The line under the device's name: "48000Hz / 24bit / 2ch".
    #[must_use]
    pub fn properties(&self) -> String {
        format!(
            "{}Hz / {}bit / {}ch",
            self.sample_rate, self.bit_depth, self.channels
        )
    }
}

/// One application's audio stream.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioStream {
    pub id: u32,
    pub app_name: String,
    /// 0.0 to 1.0.
    pub volume: f32,
    pub muted: bool,
    /// Whether the application is producing audio at all.
    pub playing: bool,
    /// The meter reading, 0.0 to 1.0.
    pub peak_level: f32,
}

impl AudioStream {
    /// A stream at the given name and volume, playing and unmuted.
    #[must_use]
    pub fn new(id: u32, app_name: &str, volume: f32) -> Self {
        Self {
            id,
            app_name: app_name.to_string(),
            volume: volume.clamp(0.0, 1.0),
            muted: false,
            playing: true,
            peak_level: 0.0,
        }
    }

    /// What actually reaches the device: nothing at all when muted.
    #[must_use]
    pub fn effective_volume(&self) -> f32 {
        if self.muted { 0.0 } else { self.volume }
    }

    /// Set the volume, clamped to the range a volume can be in.
    pub fn set_volume(&mut self, vol: f32) {
        self.volume = vol.clamp(0.0, 1.0);
    }

    /// Flip the mute.
    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
    }

    /// The volume as a whole percentage.
    #[must_use]
    pub fn volume_percent(&self) -> u8 {
        percent_of(self.volume)
    }
}

/// A 0.0-to-1.0 level as a whole percentage.
///
/// The clamp is before the cast rather than after it, because a cast out of
/// range in Rust saturates silently and a level that arrived out of range is a
/// bug worth not hiding behind a number that happens to look reasonable.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0.0..=100.0 on the line above, so the cast is exact"
)]
pub fn percent_of(level: f32) -> u8 {
    (level.clamp(0.0, 1.0) * 100.0).round().clamp(0.0, 100.0) as u8
}

// ── Selection ──────────────────────────────────────────────────────────────

/// Which column the keyboard is pointed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    /// The master column, which is always the leftmost.
    Master,
    /// A stream column, by position in the displayed order.
    Stream(usize),
}

impl Selection {
    /// The selection's position in the row of columns; master is 0.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Self::Master => 0,
            Self::Stream(i) => i.saturating_add(1),
        }
    }

    /// The selection at a position in the row of columns.
    #[must_use]
    pub fn at(index: usize) -> Self {
        match index.checked_sub(1) {
            None => Self::Master,
            Some(i) => Self::Stream(i),
        }
    }

    /// Move by a signed number of columns, wrapping at both ends.
    ///
    /// One function for both directions, because the two the old code had did
    /// not agree with each other: `move_right` from the last stream returned
    /// itself and stuck, while `move_left` from master wrapped round. Wrapping
    /// in one direction and not the other is not a decision anybody makes; it
    /// is a `match` arm nobody read.
    #[must_use]
    pub fn step(self, delta: i32, stream_count: usize) -> Self {
        let slots = stream_count.saturating_add(1);
        let Ok(slots_i) = i32::try_from(slots) else {
            return self;
        };
        let Ok(here) = i32::try_from(self.index()) else {
            return self;
        };
        let next = here.saturating_add(delta).rem_euclid(slots_i.max(1));
        #[allow(
            clippy::cast_sign_loss,
            reason = "rem_euclid on a positive modulus is non-negative"
        )]
        Self::at(next as usize)
    }
}

// ── Targets, actions and views ─────────────────────────────────────────────

/// Something on the screen a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The master fader track.
    MasterFader,
    /// The master mute button.
    MasterMute,
    /// Anywhere else in the master column.
    MasterColumn,
    /// A stream's fader track, by position in the displayed order.
    StreamFader(usize),
    /// A stream's mute button.
    StreamMute(usize),
    /// Anywhere else in a stream's column.
    StreamColumn(usize),
    /// The output device name, which opens its picker.
    OutputDevice,
    /// The input device name, which opens its picker.
    InputDevice,
    /// A row of the open output picker.
    OutputRow(usize),
    /// A row of the open input picker.
    InputRow(usize),
    /// Anywhere off an open picker.
    ClosePicker,
}

pub type Frame = guitk::frame::Frame<Target>;

/// Everything the program can be asked to do, from either input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    /// Point the keyboard at a column.
    Select(Selection),
    /// Move the selection by a signed number of columns, wrapping.
    MoveSelection(i32),
    /// Set a column's volume outright, clamped to 0.0..=1.0.
    SetVolume(Selection, f32),
    /// Move the selected column's volume by a signed step, clamped.
    NudgeVolume(f32),
    /// Flip a column's mute.
    ToggleMute(Selection),
    /// Open the output picker.
    OpenOutput,
    /// Open the input picker.
    OpenInput,
    /// Dismiss whichever picker is open.
    ClosePicker,
    /// Move the open picker's highlight by a signed number of rows, clamped.
    MovePickerRow(i32),
    /// Point the open picker at a row.
    SelectPickerRow(usize),
    /// Take the open picker's highlighted row and close it.
    ChooseDevice,
}

/// Which picker, if any, is over the mixer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picker {
    None,
    Output,
    Input,
}

/// Every key the program answers, and what it does.
///
/// Drawn along the bottom, and walked by a test in both directions — every key
/// named here answers, and every key that answers is named here — so the bar
/// cannot drift from the program the way the old one had. That one named four
/// shortcuts for a program that answered seven keys, and described `Tab` as
/// "Cycle" and `Left/Right` as "Select" when they ran the same three lines.
const SHORTCUTS: [(&str, &str); 6] = [
    ("Left/Right", "select"),
    ("Tab", "next"),
    ("Up/Down", "volume"),
    ("M", "mute"),
    ("O", "output"),
    ("I", "input"),
];

/// The keys the picker answers while it is open, drawn in its footer.
const PICKER_SHORTCUTS: [(&str, &str); 3] = [
    ("Up/Down", "choose"),
    ("Enter", "use it"),
    ("Esc", "cancel"),
];

// ── The model ──────────────────────────────────────────────────────────────

/// The mixer.
pub struct MixerApp {
    master_volume: f32,
    master_muted: bool,
    streams: Vec<AudioStream>,
    output_devices: Vec<AudioDevice>,
    selected_output: usize,
    input_devices: Vec<AudioDevice>,
    selected_input: usize,
    selection: Selection,
    picker: Picker,
    /// The row the open picker is pointed at.
    picker_row: usize,
    /// Real milliseconds banked towards the next meter step.
    meter_accum: u64,
    /// How many meter steps have been run, for anything that wants to know the
    /// meters really are advancing.
    steps: u64,
    /// The last size the window was drawn at. A click is read against this.
    size: (f32, f32),
    rng: SeededRng,
}

impl Default for MixerApp {
    fn default() -> Self {
        Self::new()
    }
}

impl MixerApp {
    /// A mixer with the stub device and stream list.
    #[must_use]
    pub fn new() -> Self {
        let streams = vec![
            AudioStream {
                id: 1,
                app_name: "Music Player".to_string(),
                volume: 0.80,
                muted: false,
                playing: true,
                peak_level: 0.72,
            },
            AudioStream {
                id: 2,
                app_name: "Firefox".to_string(),
                volume: 0.65,
                muted: false,
                playing: true,
                peak_level: 0.45,
            },
            AudioStream {
                id: 3,
                app_name: "System Sounds".to_string(),
                volume: 0.50,
                muted: false,
                playing: false,
                peak_level: 0.0,
            },
            AudioStream {
                id: 4,
                app_name: "Discord".to_string(),
                volume: 0.90,
                muted: false,
                playing: true,
                peak_level: 0.60,
            },
            AudioStream {
                id: 5,
                app_name: "Game".to_string(),
                volume: 0.75,
                muted: true,
                playing: false,
                peak_level: 0.0,
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
            picker: Picker::None,
            picker_row: 0,
            meter_accum: 0,
            steps: 0,
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
            rng: seeded_from_system(FALLBACK_SEED),
        }
    }

    // ── What the model is ──────────────────────────────────────────────────

    #[must_use]
    pub fn master_volume(&self) -> f32 {
        self.master_volume
    }

    #[must_use]
    pub fn master_muted(&self) -> bool {
        self.master_muted
    }

    /// What the master actually passes: nothing at all when muted.
    #[must_use]
    pub fn master_effective_volume(&self) -> f32 {
        if self.master_muted {
            0.0
        } else {
            self.master_volume
        }
    }

    #[must_use]
    pub fn master_volume_percent(&self) -> u8 {
        percent_of(self.master_volume)
    }

    #[must_use]
    pub fn selection(&self) -> Selection {
        self.selection
    }

    #[must_use]
    pub fn picker(&self) -> Picker {
        self.picker
    }

    #[must_use]
    pub fn picker_row(&self) -> usize {
        self.picker_row
    }

    #[must_use]
    pub fn steps(&self) -> u64 {
        self.steps
    }

    #[must_use]
    pub fn selected_output(&self) -> usize {
        self.selected_output
    }

    #[must_use]
    pub fn selected_input(&self) -> usize {
        self.selected_input
    }

    #[must_use]
    pub fn output_devices(&self) -> &[AudioDevice] {
        &self.output_devices
    }

    #[must_use]
    pub fn input_devices(&self) -> &[AudioDevice] {
        &self.input_devices
    }

    /// The streams in the order they are drawn: playing first, then by name.
    ///
    /// One function, used by the drawing *and* by everything that resolves a
    /// `Selection::Stream(i)`, so a column's position on the screen and the
    /// position the keyboard means are the same number by construction.
    #[must_use]
    pub fn sorted_streams(&self) -> Vec<&AudioStream> {
        let mut sorted: Vec<&AudioStream> = self.streams.iter().collect();
        sorted.sort_by(|a, b| {
            b.playing
                .cmp(&a.playing)
                .then_with(|| a.app_name.cmp(&b.app_name))
        });
        sorted
    }

    /// The stream ids in the order they are drawn.
    #[must_use]
    pub fn order(&self) -> Vec<u32> {
        self.sorted_streams().iter().map(|s| s.id).collect()
    }

    #[must_use]
    pub fn stream_count(&self) -> usize {
        self.streams.len()
    }

    /// The stream at a display position.
    #[must_use]
    pub fn stream_at(&self, index: usize) -> Option<&AudioStream> {
        let id = *self.order().get(index)?;
        self.streams.iter().find(|s| s.id == id)
    }

    fn stream_at_mut(&mut self, index: usize) -> Option<&mut AudioStream> {
        let id = *self.order().get(index)?;
        self.streams.iter_mut().find(|s| s.id == id)
    }

    #[must_use]
    pub fn current_output_device(&self) -> Option<&AudioDevice> {
        self.output_devices.get(self.selected_output)
    }

    #[must_use]
    pub fn current_input_device(&self) -> Option<&AudioDevice> {
        self.input_devices.get(self.selected_input)
    }

    /// The volume of a column, or `None` if there is no such column.
    #[must_use]
    pub fn volume_of(&self, sel: Selection) -> Option<f32> {
        match sel {
            Selection::Master => Some(self.master_volume),
            Selection::Stream(i) => self.stream_at(i).map(|s| s.volume),
        }
    }

    /// Whether a column is muted, or `None` if there is no such column.
    #[must_use]
    pub fn muted_of(&self, sel: Selection) -> Option<bool> {
        match sel {
            Selection::Master => Some(self.master_muted),
            Selection::Stream(i) => self.stream_at(i).map(|s| s.muted),
        }
    }

    /// How many rows the open picker is showing.
    #[must_use]
    pub fn picker_len(&self) -> usize {
        match self.picker {
            Picker::None => 0,
            Picker::Output => self.output_devices.len(),
            Picker::Input => self.input_devices.len(),
        }
    }

    // ── What the model does ────────────────────────────────────────────────

    /// Do one thing, whichever input asked for it.
    ///
    /// The single body a key and a click both go through, so a fader moved with
    /// the pointer and one moved with the keyboard cannot come to mean different
    /// things.
    pub fn apply(&mut self, action: Action) {
        match action {
            Action::Select(sel) => {
                if self.exists(sel) {
                    self.selection = sel;
                }
            }
            Action::MoveSelection(delta) => {
                self.selection = self.selection.step(delta, self.streams.len());
            }
            Action::SetVolume(sel, v) => {
                let v = v.clamp(0.0, 1.0);
                match sel {
                    Selection::Master => self.master_volume = v,
                    Selection::Stream(i) => {
                        if let Some(s) = self.stream_at_mut(i) {
                            s.volume = v;
                        }
                    }
                }
            }
            Action::NudgeVolume(delta) => {
                if let Some(here) = self.volume_of(self.selection) {
                    self.apply(Action::SetVolume(self.selection, here + delta));
                }
            }
            Action::ToggleMute(sel) => match sel {
                Selection::Master => self.master_muted = !self.master_muted,
                Selection::Stream(i) => {
                    if let Some(s) = self.stream_at_mut(i) {
                        s.muted = !s.muted;
                    }
                }
            },
            Action::OpenOutput => {
                self.picker = Picker::Output;
                self.picker_row = self.selected_output;
            }
            Action::OpenInput => {
                self.picker = Picker::Input;
                self.picker_row = self.selected_input;
            }
            Action::ClosePicker => {
                self.picker = Picker::None;
                self.picker_row = 0;
            }
            Action::MovePickerRow(delta) => {
                let len = self.picker_len();
                if len == 0 {
                    return;
                }
                let (Ok(here), Ok(last)) = (
                    i32::try_from(self.picker_row),
                    i32::try_from(len.saturating_sub(1)),
                ) else {
                    return;
                };
                #[allow(
                    clippy::cast_sign_loss,
                    reason = "clamped to 0..=last, both non-negative"
                )]
                {
                    self.picker_row = here.saturating_add(delta).clamp(0, last) as usize;
                }
            }
            Action::SelectPickerRow(row) => {
                if row < self.picker_len() {
                    self.picker_row = row;
                }
            }
            Action::ChooseDevice => {
                if self.picker_row >= self.picker_len() {
                    return;
                }
                match self.picker {
                    Picker::None => {}
                    Picker::Output => self.selected_output = self.picker_row,
                    Picker::Input => self.selected_input = self.picker_row,
                }
                self.apply(Action::ClosePicker);
            }
        }
    }

    /// Whether a column exists to be selected.
    fn exists(&self, sel: Selection) -> bool {
        match sel {
            Selection::Master => true,
            Selection::Stream(i) => i < self.streams.len(),
        }
    }

    /// Whether anything on screen is still moving, and so whether a clock is
    /// worth running.
    ///
    /// A meter that has fallen to silence, over streams that are all silent, has
    /// nothing left to draw — so the window is not woken up twenty-five times a
    /// second to redraw a picture that cannot change.
    #[must_use]
    pub fn meters_moving(&self) -> bool {
        self.streams
            .iter()
            .any(|s| (s.playing && !s.muted) || s.peak_level > 0.0)
    }

    /// One 40ms step of every meter.
    ///
    /// Attack fast, decay slow, which is what a peak meter does. The levels used
    /// to come from `(id * 7 + tick * 13) % 100`, which is not pseudo-randomness
    /// at all — it is a sawtooth. Measured, stream 1's meter read 20, 33, 46,
    /// 59, 72, 85, 98, 11, 24, …: a straight ramp of 13 hundredths per step with
    /// a period of exactly 100 steps, and every stream ran the identical ramp
    /// offset by 7 per id, so all the meters climbed in lockstep and reset
    /// together.
    fn step_meters(&mut self) {
        self.steps = self.steps.saturating_add(1);

        // Every draw is taken first: `self.rng` and `self.streams` are disjoint
        // fields, but drawing inside the loop body borrows both at once and the
        // borrow checker will not take that on trust.
        let draws: Vec<f32> = (0..self.streams.len())
            .map(|_| self.rng.unit_f32())
            .collect();

        for (stream, &draw) in self.streams.iter_mut().zip(draws.iter()) {
            if stream.playing && !stream.muted {
                let target = stream.volume * 0.8 * draw;
                let rate = if target > stream.peak_level {
                    0.6
                } else {
                    0.15
                };
                stream.peak_level += (target - stream.peak_level) * rate;
                stream.peak_level = stream.peak_level.clamp(0.0, 1.0);
            } else {
                stream.peak_level *= 0.85;
                if stream.peak_level < 0.01 {
                    stream.peak_level = 0.0;
                }
            }
        }
    }

    /// Advance the meters by however much real time has passed.
    ///
    /// The time is *banked*, not rounded off: a run of ticks shorter than a step
    /// still adds up to a step eventually, which is what stops a fast screen
    /// from running the meters slower than a slow one.
    pub fn tick(&mut self, elapsed_ms: u64) -> EventResult {
        self.meter_accum = self.meter_accum.saturating_add(elapsed_ms);
        let mut taken: u32 = 0;
        while self.meter_accum >= METER_STEP_MS {
            if taken >= MAX_CATCHUP {
                // Dropped, not banked. Banking it pays the same backlog out on
                // the next tick and the one after, so a single stall becomes a
                // permanent limp.
                self.meter_accum = self.meter_accum.checked_rem(METER_STEP_MS).unwrap_or(0);
                break;
            }
            self.meter_accum = self.meter_accum.saturating_sub(METER_STEP_MS);
            self.step_meters();
            taken = taken.saturating_add(1);
        }
        if taken == 0 {
            EventResult::Ignored
        } else {
            EventResult::Consumed
        }
    }

    /// Remember the size the window is at, so a click can be read against it.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.size = (width.max(1.0), height.max(1.0));
    }

    #[must_use]
    pub fn size(&self) -> (f32, f32) {
        self.size
    }

    #[must_use]
    pub fn layout(&self) -> Layout {
        Layout::new(self.size.0, self.size.1, self.streams.len())
    }

    // ── Input ──────────────────────────────────────────────────────────────

    /// What a key means, or `None` if the program does not answer it.
    ///
    /// Separate from doing it, so a test can ask what the program answers
    /// without also having to undo whatever the answer was.
    #[must_use]
    pub fn action_for(&self, key: Key) -> Option<Action> {
        if self.picker != Picker::None {
            return match key {
                Key::Up => Some(Action::MovePickerRow(-1)),
                Key::Down => Some(Action::MovePickerRow(1)),
                Key::Enter => Some(Action::ChooseDevice),
                Key::Escape => Some(Action::ClosePicker),
                _ => None,
            };
        }
        match key {
            Key::Left => Some(Action::MoveSelection(-1)),
            Key::Right | Key::Tab => Some(Action::MoveSelection(1)),
            Key::Up => Some(Action::NudgeVolume(VOLUME_STEP)),
            Key::Down => Some(Action::NudgeVolume(-VOLUME_STEP)),
            Key::M => Some(Action::ToggleMute(self.selection)),
            Key::O => Some(Action::OpenOutput),
            Key::I => Some(Action::OpenInput),
            _ => None,
        }
    }

    /// A key event.
    pub fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        // The release half of every keystroke. Swallowing this field is what
        // ran every key in `apps/life` twice; the shape is cheap to get right
        // and expensive to debug.
        if !ev.pressed {
            return EventResult::Ignored;
        }
        // Shift is not a modifier that refuses a key: it is half of Shift-Tab,
        // which is the one keystroke every toolkit agrees means "backwards".
        if ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.super_key {
            return EventResult::Ignored;
        }
        if ev.key == Key::Tab && ev.modifiers.shift && self.picker == Picker::None {
            self.apply(Action::MoveSelection(-1));
            return EventResult::Consumed;
        }
        match self.action_for(ev.key) {
            Some(action) => {
                self.apply(action);
                EventResult::Consumed
            }
            None => EventResult::Ignored,
        }
    }

    /// A mouse event.
    ///
    /// The boxes are the ones the drawing pass recorded, so what a click does is
    /// decided by where the thing it hit was actually drawn — there is no second
    /// copy of the layout for the pointer to disagree with.
    pub fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        let f = self.frame(self.size.0, self.size.1);
        let Some((target, rect)) = hit_with_rect(&f, ev.x, ev.y) else {
            return EventResult::Ignored;
        };
        match target {
            Target::MasterFader => {
                self.apply(Action::Select(Selection::Master));
                self.apply(Action::SetVolume(Selection::Master, value_at(rect, ev.y)));
            }
            Target::MasterMute => {
                self.apply(Action::Select(Selection::Master));
                self.apply(Action::ToggleMute(Selection::Master));
            }
            Target::MasterColumn => self.apply(Action::Select(Selection::Master)),
            Target::StreamFader(i) => {
                self.apply(Action::Select(Selection::Stream(i)));
                self.apply(Action::SetVolume(
                    Selection::Stream(i),
                    value_at(rect, ev.y),
                ));
            }
            Target::StreamMute(i) => {
                self.apply(Action::Select(Selection::Stream(i)));
                self.apply(Action::ToggleMute(Selection::Stream(i)));
            }
            Target::StreamColumn(i) => self.apply(Action::Select(Selection::Stream(i))),
            Target::OutputDevice => self.apply(Action::OpenOutput),
            Target::InputDevice => self.apply(Action::OpenInput),
            Target::OutputRow(i) | Target::InputRow(i) => {
                self.apply(Action::SelectPickerRow(i));
                self.apply(Action::ChooseDevice);
            }
            Target::ClosePicker => self.apply(Action::ClosePicker),
        }
        EventResult::Consumed
    }
}

/// The value a fader is set to by a click at `y` on the track `r`.
///
/// Up is loud, which is the only way round a fader is ever drawn.
#[must_use]
pub fn value_at(r: Rect, y: f32) -> f32 {
    if r.h <= 0.0 {
        return 0.0;
    }
    (1.0 - (y - r.y) / r.h).clamp(0.0, 1.0)
}

/// The last box recorded at a point, and the box itself.
///
/// `Frame::hit_test` answers *which* target, and a fader also needs to know how
/// far up its own track the click landed — so the box has to come back with it.
/// Reading the value from the recorded box rather than from a freshly-computed
/// one is what keeps the pointer and the drawing on the same rectangle.
#[must_use]
pub fn hit_with_rect(f: &Frame, x: f32, y: f32) -> Option<(Target, Rect)> {
    f.hits()
        .iter()
        .rev()
        .find(|(_, r)| r.contains(x, y))
        .map(|(t, r)| (*t, *r))
}

// ── Volume arithmetic ──────────────────────────────────────────────────────

/// A linear volume as decibels; silence is negative infinity, not a number.
#[must_use]
pub fn linear_to_db(linear: f32) -> f32 {
    if linear <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * linear.log10()
    }
}

/// Decibels back to a linear volume, with everything below -80dB silent.
#[must_use]
pub fn db_to_linear(db: f32) -> f32 {
    if db <= -80.0 {
        0.0
    } else {
        (10.0_f32).powf(db / 20.0).clamp(0.0, 1.0)
    }
}

/// A volume as a percentage string.
#[must_use]
pub fn format_volume_percent(volume: f32) -> String {
    format!("{}%", percent_of(volume))
}

/// A volume in decibels, to one place.
#[must_use]
pub fn format_volume_db(volume: f32) -> String {
    let db = linear_to_db(volume);
    if db == f32::NEG_INFINITY {
        "-inf dB".to_string()
    } else {
        format!("{db:.1} dB")
    }
}

/// What an application is actually heard at: its own fader through the master.
#[must_use]
pub fn combined_volume(app_volume: f32, master_volume: f32) -> f32 {
    (app_volume * master_volume).clamp(0.0, 1.0)
}

// ── Layout ─────────────────────────────────────────────────────────────────

/// The order the bands are given up in when the window is too short for all of
/// them: the shortcut bar first, then the device bar. The columns are the
/// program, so they are never dropped.
const BAND_DROP_ORDER: [usize; 2] = [1, 0];

/// The share of the window height the columns keep for themselves whatever else
/// has to go.
const COLUMNS_SHARE: f32 = 0.55;

/// Every rectangle in the window, derived from the window's own size and the
/// number of streams there are.
///
/// Built fresh on every frame and never stored on the model. The old file had no
/// layout at all — every dimension was a constant inside a `Style` in a flex
/// tree the program never measured — which is why a click had nothing to be
/// read against and `handle_slider_click` could not be given its arguments by
/// anything.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    /// The output and input device bar along the top.
    pub devices: Rect,
    /// The row of columns.
    pub band: Rect,
    /// The shortcut bar along the bottom.
    pub shortcuts: Rect,
    /// The open picker's sheet.
    pub sheet: Rect,
    /// The width of one column, master or stream.
    pub col_w: f32,
    /// The gap between two columns.
    pub gap: f32,
    /// How many stream columns there are, so `column` can refuse to invent one.
    pub cols: usize,
    pub font: f32,
    pub big: f32,
    pub pad: f32,
}

impl Layout {
    /// The layout for a window of the given size with `cols` stream columns.
    #[must_use]
    pub fn new(width: f32, height: f32, cols: usize) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let font = (h / 40.0).clamp(8.0, 15.0);
        let big = (font * 1.4).clamp(11.0, 22.0);
        // A margin may never be more than a quarter of the side it is taken
        // from, twice over: a floor of two pixels is taller than a 1x1 window,
        // and a margin that does not fit inside the thing it is a margin of puts
        // the content it indents outside the window entirely.
        let pad = (w.min(h) * 0.015).clamp(2.0, 12.0).min(w.min(h) / 4.0);

        // What each band would like, in [devices, shortcuts] order.
        let mut wants = [(h * 0.10).clamp(24.0, 52.0), (h * 0.06).clamp(18.0, 30.0)];
        let budget = (h - h * COLUMNS_SHARE - pad * 2.0).max(0.0);
        for &i in &BAND_DROP_ORDER {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [dev_h, sc_h] = wants;

        // A dropped band is `Rect::EMPTY`, not a full-width strip nought pixels
        // tall. Both read the same to `shows`, but only one of them reads the
        // same to anything asking "is this band gone, or merely thin?"
        let devices = if dev_h > 0.0 {
            Rect::new(0.0, 0.0, w, dev_h)
        } else {
            Rect::EMPTY
        };
        let shortcuts = if sc_h > 0.0 {
            Rect::new(0.0, h - sc_h, w, sc_h)
        } else {
            Rect::EMPTY
        };

        // From the heights, not from `devices.bottom()`: a dropped band's bottom
        // is zero, which is right by accident today and wrong the moment
        // `BAND_DROP_ORDER` is reordered.
        let top = dev_h;
        let bottom = if sc_h > 0.0 { h - sc_h } else { h };
        let band = Rect::new(
            pad,
            top + pad,
            (w - pad * 2.0).max(0.0),
            (bottom - top - pad * 2.0).max(0.0),
        );

        // Master plus one column per stream, with a gap between each pair.
        let slots = cols.saturating_add(1);
        // The lower clamp is a floor, and a floor can be wider than the room the
        // gaps have to share between them: at 1x1 the band is half a pixel wide
        // and five two-pixel gaps wanted ten, which pushed the first stream
        // column's origin to x=2.25 in a one-pixel window — outside the band it
        // is a gap *within*. The gaps may take at most half the band, which
        // leaves the other half to be divided among the columns and makes the
        // last column's right edge land exactly on the band's.
        let gap = (band.w * 0.012)
            .clamp(2.0, 14.0)
            .min(band.w / (2.0 * cols.max(1) as f32));
        let gaps = gap * cols as f32;
        let col_w = ((band.w - gaps) / slots as f32).max(0.0);

        let sheet_w = (w * 0.8).min(300.0);
        let sheet_h = (h * 0.8).min(260.0);
        let sheet = Rect::new((w - sheet_w) / 2.0, (h - sheet_h) / 2.0, sheet_w, sheet_h);

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            devices,
            band,
            shortcuts,
            sheet,
            col_w,
            gap,
            cols,
            font,
            big,
            pad,
        }
    }

    /// The master column.
    #[must_use]
    pub fn master(&self) -> Rect {
        Rect::new(self.band.x, self.band.y, self.col_w, self.band.h)
    }

    /// The `i`th stream column, or `None` if there is no such stream.
    ///
    /// Returning `None` rather than a rectangle off the end matters: a caller
    /// that is handed a plausible-looking box for a column that does not exist
    /// records a hit box for a stream nobody can see.
    #[must_use]
    pub fn column(&self, i: usize) -> Option<Rect> {
        if i >= self.cols {
            return None;
        }
        let n = i.saturating_add(1) as f32;
        Some(Rect::new(
            self.band.x + n * (self.col_w + self.gap),
            self.band.y,
            self.col_w,
            self.band.h,
        ))
    }

    /// The column a selection names.
    #[must_use]
    pub fn column_of(&self, sel: Selection) -> Option<Rect> {
        match sel {
            Selection::Master => Some(self.master()),
            Selection::Stream(i) => self.column(i),
        }
    }

    /// The name strip at the top of a column.
    #[must_use]
    pub fn name_of(&self, col: Rect) -> Rect {
        Rect::new(col.x, col.y, col.w, (col.h * 0.11).min(self.big * 1.4))
    }

    /// The mute button at the foot of a column.
    #[must_use]
    pub fn mute_of(&self, col: Rect) -> Rect {
        let hgt = (col.h * 0.10).clamp(0.0, 30.0);
        let inset = (col.w * 0.14).min(10.0);
        Rect::new(
            col.x + inset,
            (col.bottom() - hgt).max(col.y),
            (col.w - inset * 2.0).max(0.0),
            hgt,
        )
    }

    /// The percentage readout above the mute button.
    #[must_use]
    pub fn readout_of(&self, col: Rect) -> Rect {
        let mute = self.mute_of(col);
        let hgt = (col.h * 0.09).min(self.big * 1.3);
        Rect::new(col.x, (mute.y - hgt).max(col.y), col.w, hgt)
    }

    /// Everything between the name and the readout: the fader and the meter.
    #[must_use]
    pub fn middle_of(&self, col: Rect) -> Rect {
        let top = self.name_of(col).bottom();
        let bottom = self.readout_of(col).y;
        Rect::new(col.x, top, col.w, (bottom - top).max(0.0))
    }

    /// The fader track of a column.
    #[must_use]
    pub fn fader_of(&self, col: Rect) -> Rect {
        let mid = self.middle_of(col);
        let (fw, mw, inner) = self.middle_widths(mid);
        let x0 = mid.x + (mid.w - (fw + mw + inner)) / 2.0;
        Rect::new(x0, mid.y, fw, mid.h)
    }

    /// The peak meter of a column, beside its fader.
    #[must_use]
    pub fn meter_of(&self, col: Rect) -> Rect {
        let mid = self.middle_of(col);
        let (fw, mw, inner) = self.middle_widths(mid);
        let x0 = mid.x + (mid.w - (fw + mw + inner)) / 2.0;
        Rect::new(x0 + fw + inner, mid.y, mw, mid.h)
    }

    /// The fader width, the meter width and the gap between them.
    fn middle_widths(&self, mid: Rect) -> (f32, f32, f32) {
        let inner = (mid.w * 0.08).clamp(0.0, 8.0);
        let usable = (mid.w - inner).max(0.0);
        let fw = usable * 0.62;
        (fw, (usable - fw).max(0.0), inner)
    }

    /// The output device box in the device bar.
    #[must_use]
    pub fn output_box(&self) -> Rect {
        self.device_half(0)
    }

    /// The input device box in the device bar.
    #[must_use]
    pub fn input_box(&self) -> Rect {
        self.device_half(1)
    }

    fn device_half(&self, which: usize) -> Rect {
        if self.devices.is_empty() {
            return Rect::EMPTY;
        }
        let inset = self.pad;
        let usable = (self.devices.w - inset * 3.0).max(0.0);
        let half = usable / 2.0;
        Rect::new(
            self.devices.x + inset + which as f32 * (half + inset),
            self.devices.y + inset / 2.0,
            half,
            (self.devices.h - inset).max(0.0),
        )
    }

    /// The `i`th row of an open picker showing `len` devices.
    #[must_use]
    pub fn picker_row(&self, i: usize, len: usize) -> Option<Rect> {
        if i >= len || len == 0 {
            return None;
        }
        let head = (self.sheet.h * 0.16).min(self.big * 2.0);
        let foot = (self.sheet.h * 0.14).min(self.big * 1.8);
        let body = (self.sheet.h - head - foot).max(0.0);
        let row_h = body / len as f32;
        Some(Rect::new(
            self.sheet.x + self.pad,
            self.sheet.y + head + i as f32 * row_h,
            (self.sheet.w - self.pad * 2.0).max(0.0),
            row_h,
        ))
    }

    /// Whether a band has room to say anything.
    #[must_use]
    pub fn shows(&self, band: Rect) -> bool {
        band.h >= 10.0 && band.w >= 50.0
    }
}

// ── Drawing helpers ────────────────────────────────────────────────────────

fn fill(f: &mut Frame, r: Rect, color: Color, radius: f32) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii: CornerRadii::all(radius),
    });
}

fn stroke(f: &mut Frame, r: Rect, color: Color, line_width: f32, radius: f32) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    f.push(RenderCommand::StrokeRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        line_width,
        corner_radii: CornerRadii::all(radius),
    });
}

#[allow(clippy::too_many_arguments)]
fn label(
    f: &mut Frame,
    x: f32,
    y: f32,
    text_str: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
    max_width: Option<f32>,
) {
    if size <= 0.0 || text_str.is_empty() || max_width.is_some_and(|w| w <= 0.0) {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: text_str.to_string(),
        color,
        font_size: size,
        font_weight: weight,
        max_width,
        overflow: TextOverflow::Ellipsis,
    });
}

/// A string centred in `r`, horizontally and vertically.
fn centred_in(f: &mut Frame, r: Rect, s: &str, size: f32, color: Color, weight: FontWeightHint) {
    if r.w <= 0.0 || r.h <= 0.0 || size <= 0.0 {
        return;
    }
    let w = text::measure(s, size, weight);
    let line_h = text::line_height(size, weight);
    // Centring moves the start left, so the width to fit in has to be measured
    // from the start actually chosen, not from the box's — passing the box's
    // whole width from a start half a box to its left puts the ellipsis point
    // half a box past the box's right edge, which is a promise to clip that
    // clips nothing. And a string too long to centre must start *at* the box
    // rather than left of it, or the ellipsis trims the end of a string whose
    // beginning has already fallen off the other side.
    let x = (r.x + (r.w - w) / 2.0).max(r.x);
    label(
        f,
        x,
        r.y + (r.h - line_h) / 2.0,
        s,
        size,
        color,
        weight,
        Some((r.right() - x).max(0.0)),
    );
}

/// A string starting at the left of `r`, vertically centred and clipped to it.
fn left_in(f: &mut Frame, r: Rect, s: &str, size: f32, color: Color, weight: FontWeightHint) {
    if r.w <= 0.0 || r.h <= 0.0 || size <= 0.0 {
        return;
    }
    let line_h = text::line_height(size, weight);
    label(
        f,
        r.x,
        r.y + (r.h - line_h) / 2.0,
        s,
        size,
        color,
        weight,
        Some(r.w),
    );
}

/// The colour a level is drawn in: green until it is loud, then amber, then red.
fn level_color(level: f32) -> Color {
    if level >= 0.9 {
        RED
    } else if level >= 0.7 {
        YELLOW
    } else {
        GREEN
    }
}

// ── Drawing ────────────────────────────────────────────────────────────────

impl MixerApp {
    /// The whole window, and every hit box in it.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height, self.streams.len());
        let mut f = Frame::new(width, height);
        fill(&mut f, l.window, BASE, 0.0);

        self.draw_devices(&mut f, &l);
        self.draw_columns(&mut f, &l);
        self.draw_shortcuts(&mut f, &l);
        if self.picker != Picker::None {
            self.draw_picker(&mut f, &l);
        }
        f
    }

    fn draw_devices(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.devices) {
            return;
        }
        fill(f, l.devices, MANTLE, 0.0);
        for (which, target) in [(0usize, Target::OutputDevice), (1, Target::InputDevice)] {
            let r = l.device_half(which);
            if r.w <= 0.0 || r.h <= 0.0 {
                continue;
            }
            let (caption, device, accent) = if which == 0 {
                ("Output", self.current_output_device(), BLUE)
            } else {
                ("Input", self.current_input_device(), TEAL)
            };
            let open = matches!(
                (which, self.picker),
                (0, Picker::Output) | (1, Picker::Input)
            );
            fill(f, r, if open { SURFACE1 } else { SURFACE0 }, 5.0);
            f.hit(target, r);

            let inset = (r.w * 0.03).min(8.0);
            let inner = Rect::new(
                r.x + inset,
                r.y,
                (r.w - inset * 2.0).max(0.0),
                (r.h / 2.0).max(0.0),
            );
            let cap_w = (inner.w * 0.3).min(text::measure(caption, l.font, FontWeightHint::Bold));
            left_in(
                f,
                Rect::new(inner.x, inner.y, cap_w, inner.h),
                caption,
                l.font,
                SUBTEXT0,
                FontWeightHint::Bold,
            );
            let name = device.map_or("None", |d| d.name.as_str());
            left_in(
                f,
                Rect::new(
                    inner.x + cap_w + inset,
                    inner.y,
                    (inner.w - cap_w - inset).max(0.0),
                    inner.h,
                ),
                name,
                l.font,
                accent,
                FontWeightHint::Bold,
            );
            if let Some(d) = device {
                left_in(
                    f,
                    Rect::new(inner.x, r.y + r.h / 2.0, inner.w, (r.h / 2.0).max(0.0)),
                    &d.properties(),
                    (l.font * 0.82).max(1.0),
                    OVERLAY1,
                    FontWeightHint::Regular,
                );
            }
        }
    }

    fn draw_columns(&self, f: &mut Frame, l: &Layout) {
        if l.band.w <= 0.0 || l.band.h <= 0.0 {
            return;
        }
        self.draw_column(
            f,
            l,
            l.master(),
            Selection::Master,
            "Master",
            self.master_volume,
            self.master_muted,
            None,
            LAVENDER,
        );
        for i in 0..self.streams.len() {
            let (Some(col), Some(stream)) = (l.column(i), self.stream_at(i)) else {
                continue;
            };
            self.draw_column(
                f,
                l,
                col,
                Selection::Stream(i),
                &stream.app_name,
                stream.volume,
                stream.muted,
                Some(stream.peak_level),
                if stream.playing { BLUE } else { OVERLAY1 },
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_column(
        &self,
        f: &mut Frame,
        l: &Layout,
        col: Rect,
        sel: Selection,
        name: &str,
        volume: f32,
        muted: bool,
        peak: Option<f32>,
        accent: Color,
    ) {
        if col.w <= 0.0 || col.h <= 0.0 {
            return;
        }
        let selected = self.selection == sel;
        fill(f, col, if selected { SURFACE0 } else { MANTLE }, 6.0);
        if selected {
            stroke(f, col, accent, 2.0, 6.0);
        }
        // The whole column first, so the fader and the mute button recorded
        // below it win at the points they cover. `hit_test` takes the last box
        // at a point, which is what makes this work with no special case in the
        // handler.
        f.hit(
            match sel {
                Selection::Master => Target::MasterColumn,
                Selection::Stream(i) => Target::StreamColumn(i),
            },
            col,
        );

        centred_in(
            f,
            l.name_of(col),
            name,
            (l.font * 0.95).max(1.0),
            if muted { OVERLAY1 } else { TEXT_COLOR },
            FontWeightHint::Bold,
        );

        // The fader.
        let track = l.fader_of(col);
        fill(f, track, CRUST, 3.0);
        let filled = Rect::new(
            track.x,
            track.y + track.h * (1.0 - volume),
            track.w,
            track.h * volume,
        );
        fill(f, filled, if muted { SURFACE1 } else { accent }, 3.0);
        f.hit(
            match sel {
                Selection::Master => Target::MasterFader,
                Selection::Stream(i) => Target::StreamFader(i),
            },
            track,
        );

        // The meter, if this column has one. The master column does not: the
        // master is not a stream and has no level of its own to show.
        if let Some(level) = peak {
            let meter = l.meter_of(col);
            fill(f, meter, CRUST, 2.0);
            let lit = Rect::new(
                meter.x,
                meter.y + meter.h * (1.0 - level),
                meter.w,
                meter.h * level,
            );
            fill(f, lit, level_color(level), 2.0);
        }

        centred_in(
            f,
            l.readout_of(col),
            &format_volume_percent(volume),
            (l.font * 0.9).max(1.0),
            if muted { OVERLAY1 } else { SUBTEXT0 },
            FontWeightHint::Regular,
        );

        let mute = l.mute_of(col);
        fill(f, mute, if muted { RED } else { SURFACE1 }, 4.0);
        centred_in(
            f,
            mute,
            if muted { "muted" } else { "mute" },
            (mute.h * 0.5).clamp(1.0, l.font),
            if muted { CRUST } else { TEXT_COLOR },
            FontWeightHint::Bold,
        );
        f.hit(
            match sel {
                Selection::Master => Target::MasterMute,
                Selection::Stream(i) => Target::StreamMute(i),
            },
            mute,
        );
    }

    fn draw_shortcuts(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.shortcuts) {
            return;
        }
        fill(f, l.shortcuts, MANTLE, 0.0);
        let rows: &[(&str, &str)] = if self.picker == Picker::None {
            &SHORTCUTS
        } else {
            &PICKER_SHORTCUTS
        };
        let n = rows.len().max(1) as f32;
        let cell_w = (l.shortcuts.w - l.pad * 2.0).max(0.0) / n;
        let size = (l.shortcuts.h * 0.45).clamp(1.0, l.font * 0.85);
        for (i, (key, what)) in rows.iter().enumerate() {
            let cell = Rect::new(
                l.shortcuts.x + l.pad + i as f32 * cell_w,
                l.shortcuts.y,
                cell_w,
                l.shortcuts.h,
            );
            centred_in(
                f,
                cell,
                &format!("{key} {what}"),
                size,
                LAVENDER,
                FontWeightHint::Regular,
            );
        }
    }

    fn draw_picker(&self, f: &mut Frame, l: &Layout) {
        // Recorded first, over the whole window, so every box below it wins
        // where it is and a click anywhere else dismisses the sheet. This is the
        // whole of the modal behaviour: no guard in the handler, no ordering
        // rule to remember.
        fill(
            f,
            l.window,
            Color {
                r: 0,
                g: 0,
                b: 0,
                a: 150,
            },
            0.0,
        );
        f.hit(Target::ClosePicker, l.window);

        fill(f, l.sheet, MANTLE, 8.0);
        stroke(f, l.sheet, SURFACE1, 1.0, 8.0);

        let (title, devices, row_target): (&str, &[AudioDevice], fn(usize) -> Target) =
            match self.picker {
                Picker::Output | Picker::None => {
                    ("Output device", &self.output_devices, Target::OutputRow)
                }
                Picker::Input => ("Input device", &self.input_devices, Target::InputRow),
            };
        let head = (l.sheet.h * 0.16).min(l.big * 2.0);
        centred_in(
            f,
            Rect::new(l.sheet.x, l.sheet.y, l.sheet.w, head),
            title,
            l.big,
            TEXT_COLOR,
            FontWeightHint::Bold,
        );

        let current = match self.picker {
            Picker::Input => self.selected_input,
            Picker::Output | Picker::None => self.selected_output,
        };
        for (i, d) in devices.iter().enumerate() {
            let Some(r) = l.picker_row(i, devices.len()) else {
                continue;
            };
            if i == self.picker_row {
                fill(f, r, SURFACE0, 4.0);
            }
            let tick = if i == current { "* " } else { "  " };
            left_in(
                f,
                Rect::new(r.x + l.pad, r.y, (r.w - l.pad * 2.0).max(0.0), r.h),
                &format!("{tick}{}", d.name),
                l.font,
                if i == self.picker_row {
                    TEXT_COLOR
                } else {
                    SUBTEXT0
                },
                FontWeightHint::Regular,
            );
            f.hit(row_target(i), r);
        }
    }
}

// ── Window ─────────────────────────────────────────────────────────────────

/// The one body both the window and the test probe drive, so what a key does in
/// a test is what it does on a screen.
pub fn handle_event(app: &mut MixerApp, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) => app.handle_key(ev),
        Event::Mouse(ev) => app.handle_mouse(ev),
        Event::Tick { elapsed_ms } => app.tick(*elapsed_ms),
        Event::Resize { width, height } => {
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for MixerApp {
    fn title(&self) -> String {
        "Sound Mixer".to_string()
    }

    fn app_id(&self) -> String {
        "mixer".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// Asked after every event, so the meters get a clock exactly while there is
    /// something for them to show. An app that leaves this at the default gets
    /// no ticks at all — which is what this one did, with `update_peak_meters`
    /// waiting on the other side of it for a caller that never came.
    fn tick_interval(&self) -> Option<Duration> {
        self.meters_moving()
            .then(|| Duration::from_millis(METER_STEP_MS))
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match handle_event(self, event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The size the frame is drawn at is the size the next click is read
        // against — that is the whole point of storing it here.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for MixerApp {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(
            self,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }),
        )
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(self, &Event::Key(key.clone()))
    }
}

fn main() -> ExitCode {
    let mut app = MixerApp::new();
    app::launch("mixer", &mut app)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::expect_used,
    clippy::float_cmp,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use guitk::event::Modifiers;

    /// The window sizes every geometry invariant is checked at.
    ///
    /// The list is deliberately hostile at the bottom end. Two of `apps/life`'s
    /// production faults were found by nothing but an invariant asserted at 1x1
    /// and at 320x240 — not by any test aimed at either of them.
    const SIZES: [(f32, f32); 10] = [
        (WINDOW_WIDTH, WINDOW_HEIGHT),
        (1920.0, 1080.0),
        (1280.0, 400.0),
        (400.0, 1280.0),
        (640.0, 480.0),
        (320.0, 240.0),
        (200.0, 160.0),
        (120.0, 90.0),
        (60.0, 40.0),
        (1.0, 1.0),
    ];

    fn app() -> MixerApp {
        let mut a = MixerApp::new();
        a.resize(WINDOW_WIDTH, WINDOW_HEIGHT);
        a
    }

    fn key_ev(key: Key, modifiers: Modifiers, pressed: bool) -> KeyEvent {
        KeyEvent {
            key,
            pressed,
            modifiers,
            text: String::new(),
        }
    }

    /// A whole keystroke: the press and the release the compositor really sends.
    fn press(a: &mut MixerApp, key: Key) -> EventResult {
        let down = handle_event(a, &Event::Key(key_ev(key, Modifiers::NONE, true)));
        handle_event(a, &Event::Key(key_ev(key, Modifiers::NONE, false)));
        down
    }

    /// A whole keystroke with modifiers held.
    fn press_with(a: &mut MixerApp, key: Key, m: Modifiers) -> EventResult {
        let down = handle_event(a, &Event::Key(key_ev(key, m, true)));
        handle_event(a, &Event::Key(key_ev(key, m, false)));
        down
    }

    /// Click whatever the frame says is at a point, through the real event path.
    ///
    /// Raw pixels rather than `probe::click(target)`: what most of these tests
    /// check is *which* target a point reaches, and a helper that looks the
    /// target's own box up first would be answering the question with itself.
    fn click(a: &mut MixerApp, x: f32, y: f32) -> EventResult {
        handle_event(
            a,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        )
    }

    fn tick(a: &mut MixerApp, ms: u64) -> EventResult {
        handle_event(a, &Event::Tick { elapsed_ms: ms })
    }

    /// The middle of the box recorded for `target`, or `None` if it has none.
    fn where_is(f: &Frame, target: Target) -> Option<(f32, f32)> {
        f.hits()
            .iter()
            .rev()
            .find(|(t, _)| *t == target)
            .map(|(_, r)| r.centre())
    }

    /// The box recorded for `target`, or `None`.
    fn box_of(f: &Frame, target: Target) -> Option<Rect> {
        f.hits()
            .iter()
            .rev()
            .find(|(t, _)| *t == target)
            .map(|(_, r)| *r)
    }

    /// Everything about the model a test could care about, in one value.
    ///
    /// Comparing whole snapshots is what makes "this key changed *nothing*"
    /// checkable without listing the fields that were supposed to stay still —
    /// a list that is always one field out of date.
    #[derive(Clone, Debug, PartialEq)]
    struct Snap {
        master_volume: f32,
        master_muted: bool,
        streams: Vec<AudioStream>,
        selection: Selection,
        picker: Picker,
        picker_row: usize,
        selected_output: usize,
        selected_input: usize,
        steps: u64,
    }

    fn snap(a: &MixerApp) -> Snap {
        Snap {
            master_volume: a.master_volume,
            master_muted: a.master_muted,
            streams: a.streams.clone(),
            selection: a.selection,
            picker: a.picker,
            picker_row: a.picker_row,
            selected_output: a.selected_output,
            selected_input: a.selected_input,
            steps: a.steps,
        }
    }

    /// Every column's name, volume and mute, master first, in the order drawn.
    ///
    /// A column index is not a storage index — the streams are sorted for the
    /// screen — so "no other column moved" has to be said about columns. Saying
    /// it about `streams[i]` names a different stream and passes for the wrong
    /// reason, or fails for one.
    fn columns(a: &MixerApp) -> Vec<(String, f32, bool)> {
        let mut v = vec![("Master".to_string(), a.master_volume(), a.master_muted())];
        for i in 0..a.stream_count() {
            if let Some(s) = a.stream_at(i) {
                v.push((s.app_name.clone(), s.volume, s.muted));
            }
        }
        v
    }

    /// Everything about the model except what the columns are set to.
    fn rest(a: &MixerApp) -> (Selection, Picker, usize, usize, usize, u64) {
        (
            a.selection(),
            a.picker(),
            a.picker_row(),
            a.selected_output(),
            a.selected_input(),
            a.steps(),
        )
    }

    /// Every key the program answers, in either view.
    fn every_answered_key() -> Vec<Key> {
        vec![
            Key::Left,
            Key::Right,
            Key::Tab,
            Key::Up,
            Key::Down,
            Key::M,
            Key::O,
            Key::I,
            Key::Enter,
            Key::Escape,
        ]
    }

    // ── The layout ─────────────────────────────────────────────────────────

    #[test]
    fn every_band_is_inside_the_window_at_every_size() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h, 5);
            for (what, r) in [
                ("devices", l.devices),
                ("band", l.band),
                ("shortcuts", l.shortcuts),
                ("master", l.master()),
            ] {
                if r.is_empty() {
                    continue;
                }
                assert!(
                    r.x >= -0.01 && r.y >= -0.01 && r.right() <= w + 0.01 && r.bottom() <= h + 0.01,
                    "at {w}x{h} the {what} band {r:?} is not inside the window"
                );
            }
        }
    }

    #[test]
    fn the_columns_are_side_by_side_and_do_not_overlap() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h, 5);
            let mut previous = l.master();
            for i in 0..5 {
                let c = l.column(i).unwrap();
                assert!(
                    c.x >= previous.right() - 0.01,
                    "at {w}x{h} column {i} at {c:?} overlaps the one before it at {previous:?}"
                );
                assert!(
                    c.right() <= l.band.right() + 0.01,
                    "at {w}x{h} column {i} at {c:?} runs past the band {:?}",
                    l.band
                );
                previous = c;
            }
        }
    }

    #[test]
    fn there_is_no_column_for_a_stream_that_does_not_exist() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT, 5);
        assert!(l.column(4).is_some());
        assert!(
            l.column(5).is_none(),
            "a sixth column was offered for a five-stream mixer"
        );
        assert!(l.column(usize::MAX).is_none());
        assert!(
            Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT, 0)
                .column(0)
                .is_none(),
            "a mixer with no streams still offered a first stream column"
        );
    }

    #[test]
    fn the_parts_of_a_column_stack_up_without_overlapping() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h, 5);
            for col in std::iter::once(l.master()).chain((0..5).filter_map(|i| l.column(i))) {
                let name = l.name_of(col);
                let mid = l.middle_of(col);
                let readout = l.readout_of(col);
                let mute = l.mute_of(col);
                assert!(
                    name.bottom() <= mid.y + 0.01
                        && mid.bottom() <= readout.y + 0.01
                        && readout.bottom() <= mute.y + 0.01,
                    "at {w}x{h} a column's parts overlap: \
                     name {name:?} mid {mid:?} readout {readout:?} mute {mute:?}"
                );
                for (what, r) in [
                    ("name", name),
                    ("middle", mid),
                    ("readout", readout),
                    ("mute", mute),
                    ("fader", l.fader_of(col)),
                    ("meter", l.meter_of(col)),
                ] {
                    assert!(
                        r.x >= col.x - 0.01
                            && r.right() <= col.right() + 0.01
                            && r.y >= col.y - 0.01
                            && r.bottom() <= col.bottom() + 0.01,
                        "at {w}x{h} the {what} {r:?} is outside its column {col:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_fader_and_the_meter_are_side_by_side_and_do_not_overlap() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h, 5);
            let col = l.column(0).unwrap();
            let fader = l.fader_of(col);
            let meter = l.meter_of(col);
            assert!(
                meter.x >= fader.right() - 0.01,
                "at {w}x{h} the meter {meter:?} overlaps the fader {fader:?}"
            );
            assert!(
                fader.w >= meter.w - 0.01,
                "at {w}x{h} the meter {meter:?} is wider than the fader {fader:?} \
                 it is a companion to"
            );
        }
    }

    #[test]
    fn nothing_is_drawn_outside_the_window() {
        for (w, h) in SIZES {
            let mut a = app();
            a.resize(w, h);
            let f = a.frame(w, h);
            for c in f.commands() {
                let (x, y, cw, ch) = match c {
                    RenderCommand::FillRect {
                        x,
                        y,
                        width,
                        height,
                        ..
                    }
                    | RenderCommand::StrokeRect {
                        x,
                        y,
                        width,
                        height,
                        ..
                    } => (*x, *y, *width, *height),
                    _ => continue,
                };
                assert!(
                    x >= -0.01 && y >= -0.01 && x + cw <= w + 0.01 && y + ch <= h + 0.01,
                    "at {w}x{h} a rect at ({x}, {y}) {cw}x{ch} leaves the window"
                );
            }
        }
    }

    #[test]
    fn every_string_is_bounded_by_the_space_it_has() {
        for (w, h) in SIZES {
            let mut a = app();
            a.resize(w, h);
            for open in [false, true] {
                if open {
                    a.apply(Action::OpenOutput);
                }
                let f = a.frame(w, h);
                for c in f.commands() {
                    if let RenderCommand::Text {
                        x,
                        text,
                        font_size,
                        font_weight,
                        max_width,
                        ..
                    } = c
                    {
                        let mw = max_width.unwrap_or_else(|| {
                            panic!("at {w}x{h} the string {text:?} was given no width to fit in")
                        });
                        assert!(
                            x + mw <= w + 0.01,
                            "at {w}x{h} the string {text:?} may run to {} in a {w}-wide window",
                            x + mw
                        );
                        let _ = text::measure(text, *font_size, *font_weight);
                    }
                }
            }
        }
    }

    #[test]
    fn every_hit_box_is_inside_the_window() {
        for (w, h) in SIZES {
            let mut a = app();
            a.resize(w, h);
            for open in [false, true] {
                if open {
                    a.apply(Action::OpenOutput);
                }
                let f = a.frame(w, h);
                for (t, r) in f.hits() {
                    assert!(
                        r.x >= -0.01
                            && r.y >= -0.01
                            && r.right() <= w + 0.01
                            && r.bottom() <= h + 0.01,
                        "at {w}x{h} the box {r:?} for {t:?} is outside the window"
                    );
                }
            }
        }
    }

    #[test]
    fn the_frame_is_balanced_at_every_size_and_in_every_view() {
        for (w, h) in SIZES {
            for action in [Action::ClosePicker, Action::OpenOutput, Action::OpenInput] {
                let mut a = app();
                a.resize(w, h);
                a.apply(action);
                assert!(
                    a.frame(w, h).is_balanced(),
                    "at {w}x{h} after {action:?} the frame ended unbalanced"
                );
            }
        }
    }

    #[test]
    fn a_label_stays_inside_the_control_it_names() {
        // The window-level bound above cannot see this one: a centred string's
        // `max_width` is `r.right() - x`, so moving the start left grows the
        // width by exactly as much and `x + max_width` never moves. Only the
        // control's own box catches a label that has outgrown its button.
        //
        // The shortcut bar is skipped, and deliberately: its labels are not
        // clickable and so record no hit box, and giving them one to satisfy a
        // test would be inventing a target that does nothing — the phantom-state
        // fault this whole rewrite exists to remove. They are covered instead by
        // `a_centred_string_never_overflows_the_box_it_is_centred_in`, which
        // checks the helper itself rather than one of its callers.
        for (w, h) in SIZES {
            let mut a = app();
            a.resize(w, h);
            let l = a.layout();
            let f = a.frame(w, h);
            let boxes: Vec<Rect> = f.hits().iter().map(|(_, r)| *r).collect();
            for c in f.commands() {
                let RenderCommand::Text {
                    x,
                    y,
                    max_width: Some(mw),
                    text,
                    ..
                } = c
                else {
                    continue;
                };
                if !l.shortcuts.is_empty() && *y >= l.shortcuts.y - 0.01 {
                    continue;
                }
                let fits = boxes
                    .iter()
                    .any(|b| *x >= b.x - 0.01 && x + mw <= b.right() + 0.01);
                assert!(
                    fits,
                    "at {w}x{h} the string {text:?} spans {x}..{} and fits inside \
                     no control that was drawn",
                    x + mw
                );
            }
        }
    }

    #[test]
    fn a_centred_string_never_overflows_the_box_it_is_centred_in() {
        // Every label in the program goes through `centred_in` or `left_in`, so
        // the containment claim is worth checking on the helper directly rather
        // than only through whichever callers happen to have a hit box. The
        // fault it guards is real and was shipped in `apps/life`: measuring the
        // width to fit in from the *box's* start rather than the start actually
        // chosen leaves the ellipsis point outside the box, which is a promise
        // to clip that clips nothing.
        let strings = [
            "M",
            "mute",
            "Left/Right select",
            "a string far too long to fit in any of these boxes at all",
        ];
        for boxes in [
            Rect::new(0.0, 0.0, 40.0, 12.0),
            Rect::new(100.0, 50.0, 8.0, 10.0),
            Rect::new(3.5, 1.0, 300.0, 40.0),
            Rect::new(0.0, 0.0, 1.0, 1.0),
        ] {
            for s in strings {
                for size in [6.0_f32, 11.0, 22.0] {
                    let mut f = Frame::new(400.0, 400.0);
                    centred_in(&mut f, boxes, s, size, TEXT_COLOR, FontWeightHint::Regular);
                    left_in(&mut f, boxes, s, size, TEXT_COLOR, FontWeightHint::Regular);
                    for c in f.commands() {
                        let RenderCommand::Text { x, max_width, .. } = c else {
                            continue;
                        };
                        let mw = max_width.expect("a drawn string with no width to fit in");
                        assert!(
                            *x >= boxes.x - 0.01 && x + mw <= boxes.right() + 0.01,
                            "{s:?} at size {size} spans {x}..{} outside its box {boxes:?}",
                            x + mw
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_layout_is_read_from_the_window_and_not_remembered() {
        // A layout kept on the model is a layout that can disagree with the
        // window it is drawn in. `Layout::new` is the only way to get one, and
        // it takes the size as an argument, so the two cannot drift.
        let a = app();
        let small = Layout::new(400.0, 300.0, 5);
        let large = Layout::new(1600.0, 900.0, 5);
        assert_ne!(small, large);
        assert_eq!(a.layout(), Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT, 5));
        let mut b = app();
        b.resize(400.0, 300.0);
        assert_eq!(b.layout(), small);
    }

    #[test]
    fn the_first_frame_is_drawn_at_the_size_it_is_given() {
        let mut a = MixerApp::new();
        let tree = a.render(640.0, 480.0);
        assert!(!tree.commands.is_empty(), "the first frame drew nothing");
        assert_eq!(
            a.size(),
            (640.0, 480.0),
            "rendering at a size did not leave that size behind for the next click"
        );
    }

    // ── The keyboard ───────────────────────────────────────────────────────

    /// Every key on the board, so "does it answer this one?" can be asked of all
    /// of them rather than of the ones somebody remembered.
    #[rustfmt::skip]
    fn all_keys() -> Vec<Key> {
        vec![
            Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H, Key::I, Key::J,
            Key::K, Key::L, Key::M, Key::N, Key::O, Key::P, Key::Q, Key::R, Key::S, Key::T,
            Key::U, Key::V, Key::W, Key::X, Key::Y, Key::Z, Key::Num0, Key::Num1, Key::Num2,
            Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9,
            Key::F1, Key::F2, Key::F3, Key::F12, Key::Left, Key::Right, Key::Up, Key::Down,
            Key::Home, Key::End, Key::PageUp, Key::PageDown, Key::Backspace, Key::Delete,
            Key::Insert, Key::Enter, Key::Tab, Key::Escape, Key::Space, Key::Comma,
            Key::Period, Key::Slash, Key::Minus, Key::Equals, Key::Grave,
        ]
    }

    #[test]
    fn a_key_release_on_its_own_changes_nothing() {
        // Not redundant with the whole-keystroke tests, and this is worth being
        // explicit about: `press()` sends a press *and* a release, so a program
        // that acted on the release instead of the press would produce an
        // identical net result and no test built on whole keystrokes could tell
        // the two apart. Only half a keystroke can. Swallowing this field is
        // what ran every key twice in `apps/life`.
        for key in all_keys() {
            for picker in [Action::ClosePicker, Action::OpenOutput] {
                let mut a = app();
                a.apply(picker);
                let before = snap(&a);
                let r = handle_event(&mut a, &Event::Key(key_ev(key, Modifiers::NONE, false)));
                assert_eq!(
                    r,
                    EventResult::Ignored,
                    "the release of {key:?} was consumed"
                );
                assert_eq!(snap(&a), before, "the release of {key:?} changed the model");
            }
        }
    }

    #[test]
    fn a_modifier_held_down_refuses_the_key() {
        // A program that answers Ctrl-M as though it were M can never grow a
        // Ctrl shortcut, because the plain key has already claimed the chord.
        for m in [Modifiers::ctrl(), Modifiers::alt(), Modifiers::super_key()] {
            for key in all_keys() {
                let mut a = app();
                let before = snap(&a);
                let r = press_with(&mut a, key, m);
                assert_eq!(r, EventResult::Ignored, "{key:?} with {m:?} was consumed");
                assert_eq!(snap(&a), before, "{key:?} with {m:?} changed the model");
            }
        }
    }

    #[test]
    fn shift_is_not_a_modifier_that_refuses_a_key() {
        // Shift is half of Shift-Tab, so a handler that lumps it in with Ctrl
        // and Alt makes the one chord every toolkit agrees on unreachable.
        let mut a = app();
        assert_eq!(
            press_with(&mut a, Key::Tab, Modifiers::shift()),
            EventResult::Consumed,
            "Shift-Tab was refused as though Shift were Ctrl"
        );
        assert_ne!(a.selection(), Selection::Master);
    }

    #[test]
    fn tab_moves_forward_and_shift_tab_moves_back() {
        // The old `Tab` arm was a byte-for-byte copy of the `Right` arm, so the
        // shortcut bar named two keys for one behaviour and Shift-Tab did
        // nothing at all.
        let mut a = app();
        press(&mut a, Key::Tab);
        press(&mut a, Key::Tab);
        assert_eq!(a.selection(), Selection::Stream(1));
        press_with(&mut a, Key::Tab, Modifiers::shift());
        assert_eq!(
            a.selection(),
            Selection::Stream(0),
            "Shift-Tab did not move the selection back"
        );
        press_with(&mut a, Key::Tab, Modifiers::shift());
        assert_eq!(a.selection(), Selection::Master);
        press_with(&mut a, Key::Tab, Modifiers::shift());
        assert_eq!(
            a.selection(),
            Selection::Stream(4),
            "Shift-Tab off the left end did not wrap to the right end"
        );
    }

    #[test]
    fn the_selection_moves_one_column_per_keystroke_and_wraps_at_both_ends() {
        // Wrapping in one direction and not the other is what the old
        // `move_right`/`move_left` pair did: right stuck at the last stream and
        // left wrapped round. Walking a full circle in each direction is the
        // check that cannot pass for a program that only wraps one way.
        let n = app().stream_count();
        let mut a = app();
        let mut seen = vec![a.selection()];
        for _ in 0..=n {
            press(&mut a, Key::Right);
            seen.push(a.selection());
        }
        assert_eq!(
            seen,
            vec![
                Selection::Master,
                Selection::Stream(0),
                Selection::Stream(1),
                Selection::Stream(2),
                Selection::Stream(3),
                Selection::Stream(4),
                Selection::Master,
            ],
            "Right did not walk every column once and wrap back to master"
        );

        let mut b = app();
        let mut back = vec![b.selection()];
        for _ in 0..=n {
            press(&mut b, Key::Left);
            back.push(b.selection());
        }
        assert_eq!(
            back,
            vec![
                Selection::Master,
                Selection::Stream(4),
                Selection::Stream(3),
                Selection::Stream(2),
                Selection::Stream(1),
                Selection::Stream(0),
                Selection::Master,
            ],
            "Left did not walk every column once and wrap back to master"
        );
    }

    #[test]
    fn left_and_right_undo_each_other_from_every_column() {
        for start in 0..=app().stream_count() {
            let mut a = app();
            for _ in 0..start {
                press(&mut a, Key::Right);
            }
            let here = a.selection();
            press(&mut a, Key::Right);
            press(&mut a, Key::Left);
            assert_eq!(a.selection(), here, "Right then Left did not come home");
            press(&mut a, Key::Left);
            press(&mut a, Key::Right);
            assert_eq!(a.selection(), here, "Left then Right did not come home");
        }
    }

    #[test]
    fn up_and_down_move_the_selected_column_by_one_step_and_no_other() {
        for start in 0..=app().stream_count() {
            let mut a = app();
            for _ in 0..start {
                press(&mut a, Key::Right);
            }
            let sel = a.selection();
            let before = snap(&a);
            let was = a.volume_of(sel).unwrap();
            press(&mut a, Key::Up);
            assert_eq!(
                a.volume_of(sel).unwrap(),
                (was + VOLUME_STEP).clamp(0.0, 1.0),
                "Up did not move {sel:?} by one step"
            );
            // Nothing else moved: the only difference from the snapshot is the
            // one column's volume.
            let mut after = snap(&a);
            match sel {
                Selection::Master => after.master_volume = before.master_volume,
                Selection::Stream(i) => {
                    let id = a.order()[i];
                    for s in &mut after.streams {
                        if s.id == id {
                            s.volume = before.streams.iter().find(|o| o.id == id).unwrap().volume;
                        }
                    }
                }
            }
            assert_eq!(after, before, "Up changed something other than {sel:?}");

            press(&mut a, Key::Down);
            assert_eq!(
                a.volume_of(sel).unwrap(),
                was,
                "Down did not undo Up for {sel:?}"
            );
        }
    }

    #[test]
    fn the_volume_stops_at_both_ends_of_its_range() {
        for sel in [Selection::Master, Selection::Stream(2)] {
            let mut a = app();
            a.apply(Action::Select(sel));
            for _ in 0..60 {
                press(&mut a, Key::Up);
            }
            assert_eq!(a.volume_of(sel).unwrap(), 1.0, "{sel:?} ran past full");
            for _ in 0..60 {
                press(&mut a, Key::Down);
            }
            assert_eq!(a.volume_of(sel).unwrap(), 0.0, "{sel:?} ran past silence");
        }
    }

    #[test]
    fn m_mutes_the_column_the_keyboard_is_pointed_at_and_no_other() {
        for start in 0..=app().stream_count() {
            let mut a = app();
            for _ in 0..start {
                press(&mut a, Key::Right);
            }
            let sel = a.selection();
            let before: Vec<bool> = std::iter::once(a.master_muted())
                .chain((0..a.stream_count()).map(|i| a.stream_at(i).unwrap().muted))
                .collect();
            press(&mut a, Key::M);
            let after: Vec<bool> = std::iter::once(a.master_muted())
                .chain((0..a.stream_count()).map(|i| a.stream_at(i).unwrap().muted))
                .collect();
            for (i, (was, now)) in before.iter().zip(after.iter()).enumerate() {
                if i == sel.index() {
                    assert_ne!(was, now, "M did not flip the mute of {sel:?}");
                } else {
                    assert_eq!(was, now, "M flipped column {i} as well as {sel:?}");
                }
            }
        }
    }

    #[test]
    fn muting_a_column_silences_it_without_forgetting_where_its_fader_was() {
        let mut a = app();
        let was = a.master_volume();
        press(&mut a, Key::M);
        assert_eq!(a.master_effective_volume(), 0.0);
        assert_eq!(
            a.master_volume(),
            was,
            "muting moved the fader instead of gating it"
        );
        press(&mut a, Key::M);
        assert_eq!(a.master_effective_volume(), was);
    }

    #[test]
    fn a_key_the_program_does_not_answer_is_left_alone() {
        // Ignored, not consumed. The old `Escape` arm swallowed a keystroke to
        // do nothing, over a comment saying it "could close the app" — which is
        // strictly worse than ignoring it, because a swallowed key never reaches
        // the window manager that would have acted on it.
        let answered = every_answered_key();
        for key in all_keys() {
            if answered.contains(&key) {
                continue;
            }
            let mut a = app();
            let before = snap(&a);
            assert_eq!(
                press(&mut a, key),
                EventResult::Ignored,
                "{key:?} was consumed by a program that does nothing with it"
            );
            assert_eq!(snap(&a), before, "{key:?} changed the model");
        }
    }

    #[test]
    fn escape_is_left_alone_when_there_is_nothing_for_it_to_close() {
        let mut a = app();
        let before = snap(&a);
        assert_eq!(press(&mut a, Key::Escape), EventResult::Ignored);
        assert_eq!(snap(&a), before);
    }

    #[test]
    fn the_shortcut_bar_names_every_key_the_program_answers_and_no_others() {
        // Walked in both directions, so the bar cannot drift from the program in
        // either. The old bar named four shortcuts for a program that answered
        // seven keys, and described `Tab` and `Left/Right` as different things
        // when they ran the same three lines.
        fn keys_named(label: &str) -> Vec<Key> {
            match label {
                "Left/Right" => vec![Key::Left, Key::Right],
                "Up/Down" => vec![Key::Up, Key::Down],
                "Tab" => vec![Key::Tab],
                "M" => vec![Key::M],
                "O" => vec![Key::O],
                "I" => vec![Key::I],
                "Enter" => vec![Key::Enter],
                "Esc" => vec![Key::Escape],
                other => panic!("the shortcut bar names {other:?}, which is not a key"),
            }
        }

        for (open, rows) in [
            (Action::ClosePicker, &SHORTCUTS[..]),
            (Action::OpenOutput, &PICKER_SHORTCUTS[..]),
        ] {
            let mut a = app();
            a.apply(open);
            let mut named: Vec<Key> = rows.iter().flat_map(|(k, _)| keys_named(k)).collect();
            named.sort_by_key(|k| format!("{k:?}"));

            for key in &named {
                assert!(
                    a.action_for(*key).is_some(),
                    "after {open:?} the bar names {key:?}, which the program does not answer"
                );
            }
            let mut answered: Vec<Key> = all_keys()
                .into_iter()
                .filter(|k| a.action_for(*k).is_some())
                .collect();
            answered.sort_by_key(|k| format!("{k:?}"));
            assert_eq!(
                answered, named,
                "after {open:?} the keys the program answers are not the keys the bar names"
            );
        }
    }

    #[test]
    fn the_shortcut_bar_says_what_view_it_is_in() {
        for (open, expect, forbid) in [
            (Action::ClosePicker, "mute", "cancel"),
            (Action::OpenOutput, "cancel", "mute"),
        ] {
            let mut a = app();
            a.apply(open);
            let f = a.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
            let l = a.layout();
            // Only the strings drawn in the shortcut band count. The columns'
            // own "mute" buttons are still drawn underneath the picker's
            // backdrop — reading the whole frame would find the word there and
            // conclude the bar had said it, which is a test that passes on text
            // the bar never wrote.
            let text: String = f
                .commands()
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { text, y, .. } if *y >= l.shortcuts.y - 0.01 => {
                        Some(text.as_str())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" | ");
            assert!(
                text.contains(expect),
                "after {open:?} the bar never said {expect:?}: {text}"
            );
            assert!(
                !text.contains(forbid),
                "after {open:?} the bar still said {forbid:?}: {text}"
            );
        }
    }

    // ── The pointer ────────────────────────────────────────────────────────

    #[test]
    fn only_a_left_press_does_anything() {
        // The old file had no `Event::Mouse` arm at all, so there was nothing to
        // be wrong about which button did what. Now that there is one, every
        // other button and the release of the answered one must leave the model
        // exactly as they found it.
        let f = app().frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let (x, y) = where_is(&f, Target::StreamMute(2)).expect("a mute button to press");
        for kind in [
            MouseEventKind::Press(MouseButton::Right),
            MouseEventKind::Press(MouseButton::Middle),
            MouseEventKind::Release(MouseButton::Left),
            MouseEventKind::Release(MouseButton::Right),
            MouseEventKind::Move,
        ] {
            let mut a = app();
            let before = snap(&a);
            let named = format!("{kind:?}");
            let r = handle_event(&mut a, &Event::Mouse(MouseEvent { x, y, kind }));
            assert_eq!(r, EventResult::Ignored, "{named} was consumed");
            assert_eq!(snap(&a), before, "{named} changed the model");
        }
        // …and the left press at the same point does do something, so the test
        // above is not passing because the point is dead.
        let mut a = app();
        assert_eq!(click(&mut a, x, y), EventResult::Consumed);
        assert_ne!(snap(&a), snap(&app()));
    }

    #[test]
    fn a_click_on_nothing_is_left_alone() {
        // Outside every recorded box. A program that consumed it would be
        // telling the compositor it had handled a click it did nothing about.
        let mut a = app();
        let before = snap(&a);
        for (x, y) in [(-5.0, -5.0), (WINDOW_WIDTH + 40.0, 10.0), (10.0, -1.0)] {
            assert_eq!(
                click(&mut a, x, y),
                EventResult::Ignored,
                "a click at ({x}, {y}) was consumed"
            );
        }
        assert_eq!(snap(&a), before);
    }

    #[test]
    fn a_click_on_a_column_selects_it_and_changes_nothing_else() {
        let f = app().frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        for (target, sel) in [
            (Target::MasterColumn, Selection::Master),
            (Target::StreamColumn(0), Selection::Stream(0)),
            (Target::StreamColumn(3), Selection::Stream(3)),
        ] {
            let col = box_of(&f, target).expect("a column to click");
            let name = app().layout().name_of(col);
            let (x, y) = name.centre();
            let mut a = app();
            let mut before = snap(&a);
            assert_eq!(click(&mut a, x, y), EventResult::Consumed);
            assert_eq!(a.selection(), sel, "clicking {target:?} did not select it");
            before.selection = sel;
            assert_eq!(snap(&a), before, "clicking {target:?} did more than select");
        }
    }

    #[test]
    fn a_click_on_a_fader_sets_the_volume_to_the_height_it_landed_at() {
        // The value comes from the box the drawing pass recorded, so this also
        // says the fader's pixels and its arithmetic are the same rectangle.
        let f = app().frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        for (target, sel) in [
            (Target::MasterFader, Selection::Master),
            (Target::StreamFader(1), Selection::Stream(1)),
            (Target::StreamFader(4), Selection::Stream(4)),
        ] {
            let track = box_of(&f, target).expect("a fader to click");
            for (frac, want) in [(0.0_f32, 1.0_f32), (0.25, 0.75), (0.5, 0.5), (1.0, 0.0)] {
                let mut a = app();
                // A hair inside the bottom edge, which is exclusive.
                let y = (track.y + track.h * frac).min(track.bottom() - 0.01);
                assert_eq!(click(&mut a, track.centre().0, y), EventResult::Consumed);
                let got = a.volume_of(sel).expect("the column to have a volume");
                assert!(
                    (got - want).abs() < 0.02,
                    "{target:?} clicked {frac} of the way down set {got}, wanted {want}"
                );
                assert_eq!(a.selection(), sel, "clicking a fader did not select it too");
            }
        }
    }

    #[test]
    fn a_fader_click_moves_the_column_it_is_on_and_no_other() {
        let f = app().frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let track = box_of(&f, Target::StreamFader(2)).expect("a fader");
        let mut a = app();
        let before = columns(&a);
        let (x, y) = track.centre();
        click(&mut a, x, y);
        let after = columns(&a);
        for (i, (was, now)) in before.iter().zip(after.iter()).enumerate() {
            if i == Selection::Stream(2).index() {
                assert_eq!(was.0, now.0, "the columns changed order");
                assert!(
                    (now.1 - 0.5).abs() < 0.02,
                    "the middle of the track is half volume, not {}",
                    now.1
                );
                assert_eq!(was.2, now.2, "a fader click muted the column");
            } else {
                assert_eq!(
                    was, now,
                    "column {i} moved when column 2's fader was clicked"
                );
            }
        }
        assert_eq!(before.len(), after.len(), "a column came or went");
        assert_eq!(rest(&a), (Selection::Stream(2), Picker::None, 0, 0, 0, 0));
    }

    #[test]
    fn a_click_on_a_mute_button_mutes_that_column_and_no_other() {
        let f = app().frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        for (target, sel) in [
            (Target::MasterMute, Selection::Master),
            (Target::StreamMute(0), Selection::Stream(0)),
            (Target::StreamMute(4), Selection::Stream(4)),
        ] {
            let (x, y) = where_is(&f, target).expect("a mute button");
            let mut a = app();
            let was = a.muted_of(sel).expect("a column to be muted");
            let before = columns(&a);
            assert_eq!(click(&mut a, x, y), EventResult::Consumed);
            assert_eq!(
                a.muted_of(sel),
                Some(!was),
                "{target:?} did not toggle mute"
            );
            let after = columns(&a);
            assert_eq!(before.len(), after.len(), "a column came or went");
            for (i, (b, n)) in before.iter().zip(after.iter()).enumerate() {
                if i == sel.index() {
                    assert_eq!(
                        (&b.0, b.1, !b.2),
                        (&n.0, n.1, n.2),
                        "{target:?} did more than mute its own column"
                    );
                } else {
                    assert_eq!(b, n, "{target:?} changed column {i} as well");
                }
            }
            assert_eq!(
                rest(&a),
                (sel, Picker::None, 0, 0, 0, 0),
                "{target:?} did more than mute"
            );
        }
    }

    #[test]
    fn the_pixels_a_control_is_drawn_on_are_the_pixels_that_reach_it() {
        // Walk the window on a coarse grid and ask, for every point, that the
        // target the frame reports is the one whose drawn box contains it —
        // taking the last such box, which is the rule `hit_test` itself uses.
        for (w, h) in SIZES {
            let a = app();
            let f = a.frame(w, h);
            let hits = f.hits().to_vec();
            let step = (w / 23.0).max(0.5);
            let vstep = (h / 23.0).max(0.5);
            let mut x = 0.25;
            while x < w {
                let mut y = 0.25;
                while y < h {
                    let want = hits
                        .iter()
                        .rev()
                        .find(|(_, r)| r.contains(x, y))
                        .map(|p| p.0);
                    let got = hit_with_rect(&f, x, y).map(|p| p.0);
                    assert_eq!(got, want, "at {w}x{h} the point ({x}, {y}) disagreed");
                    y += vstep;
                }
                x += step;
            }
        }
    }

    #[test]
    fn every_column_can_be_muted_and_faded_with_the_pointer() {
        // Not just the three the tests above happen to name: every column the
        // program draws, so a column that exists but records no box is caught.
        let a = app();
        let f = a.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let mut wanted = vec![
            Target::MasterColumn,
            Target::MasterFader,
            Target::MasterMute,
        ];
        for i in 0..a.stream_count() {
            wanted.push(Target::StreamColumn(i));
            wanted.push(Target::StreamFader(i));
            wanted.push(Target::StreamMute(i));
        }
        for t in wanted {
            let (x, y) = where_is(&f, t).unwrap_or_else(|| panic!("{t:?} records no hit box"));
            let mut b = app();
            let before = snap(&b);
            assert_eq!(click(&mut b, x, y), EventResult::Consumed, "{t:?}");
            if !matches!(t, Target::MasterColumn) {
                assert_ne!(snap(&b), before, "clicking {t:?} did nothing at all");
            }
        }
    }

    #[test]
    fn the_device_bars_open_the_picker_they_name() {
        for (target, want) in [
            (Target::OutputDevice, Picker::Output),
            (Target::InputDevice, Picker::Input),
        ] {
            let mut a = app();
            let f = a.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
            let (x, y) = where_is(&f, target).expect("a device bar");
            assert_eq!(a.picker(), Picker::None);
            assert_eq!(click(&mut a, x, y), EventResult::Consumed);
            assert_eq!(a.picker(), want, "{target:?} opened the wrong picker");
        }
    }

    // ── The clock ──────────────────────────────────────────────────────────

    #[test]
    fn the_window_is_asked_for_a_clock_exactly_while_the_meters_are_moving() {
        // The whole of lesson 47: the old `main` called `update_peak_meters()`
        // ten times in a loop and then exited, because nothing was ever going to
        // call it again. A window only ticks an app that asks for a tick.
        let mut a = app();
        assert!(a.meters_moving(), "the mixer starts with streams playing");
        assert_eq!(
            a.tick_interval(),
            Some(Duration::from_millis(METER_STEP_MS)),
            "a mixer with moving meters asked for no clock"
        );

        // Silence everything and let the decay run out.
        for i in 0..a.stream_count() {
            a.apply(Action::ToggleMute(Selection::Stream(i)));
        }
        for _ in 0..400 {
            tick(&mut a, METER_STEP_MS);
        }
        assert!(!a.meters_moving(), "the meters never came to rest");
        assert_eq!(
            a.tick_interval(),
            None,
            "a still mixer still asked to be woken 25 times a second"
        );
    }

    #[test]
    fn a_tick_advances_by_the_time_that_passed_not_by_the_interval() {
        // The old `update_peak_meters` took no elapsed time at all, so the
        // ballistics were tied to the frame rate: the same attack per *call*.
        for (ms, want) in [
            (METER_STEP_MS, 1_u64),
            (METER_STEP_MS * 3, 3),
            (METER_STEP_MS * 3 + 5, 3),
        ] {
            let mut a = app();
            assert_eq!(tick(&mut a, ms), EventResult::Consumed);
            assert_eq!(a.steps(), want, "{ms}ms should be {want} meter step(s)");
        }
    }

    #[test]
    fn a_tick_shorter_than_a_step_banks_the_time_rather_than_losing_it() {
        let mut a = app();
        let each = METER_STEP_MS / 4;
        for i in 1..4 {
            assert_eq!(
                tick(&mut a, each),
                EventResult::Ignored,
                "a tick that ran nothing claimed the frame changed"
            );
            assert_eq!(a.steps(), 0, "{i} quarter-steps ran a step early");
        }
        assert_eq!(tick(&mut a, each), EventResult::Consumed);
        assert_eq!(
            a.steps(),
            1,
            "four quarter-steps did not add up to one step"
        );
    }

    #[test]
    fn catching_up_is_capped_and_the_backlog_is_dropped_not_banked() {
        // A window that was away for an hour must not run an hour of meters, and
        // must not owe them either — banked backlog turns one stall into a
        // permanent limp.
        let mut a = app();
        tick(&mut a, METER_STEP_MS * 1000);
        assert_eq!(
            a.steps(),
            u64::from(MAX_CATCHUP),
            "the catch-up loop is not capped"
        );
        let after_stall = a.steps();
        tick(&mut a, METER_STEP_MS);
        assert_eq!(
            a.steps(),
            after_stall.saturating_add(1),
            "the dropped backlog was banked and paid out on the next tick"
        );
    }

    #[test]
    fn a_meter_rises_towards_a_playing_stream_and_falls_to_silence_on_a_muted_one() {
        let mut a = app();
        // A muted stream's meter must fall all the way to nothing, not to a
        // floor: `< 0.01` is snapped to zero so `meters_moving` can go false.
        let silent = (0..a.stream_count())
            .find(|i| a.stream_at(*i).is_some_and(|s| s.peak_level > 0.0))
            .expect("a stream with a live meter");
        a.apply(Action::ToggleMute(Selection::Stream(silent)));
        for _ in 0..200 {
            tick(&mut a, METER_STEP_MS);
        }
        assert_eq!(
            a.stream_at(silent).map(|s| s.peak_level),
            Some(0.0),
            "a muted meter never reached silence"
        );

        // And a playing one keeps moving: over a long run its meter is not the
        // one constant value a sawtooth or a dead loop would leave it at.
        let mut b = app();
        let playing = (0..b.stream_count())
            .find(|i| b.stream_at(*i).is_some_and(|s| s.playing && !s.muted))
            .expect("a playing stream");
        let mut seen: Vec<u8> = Vec::new();
        for _ in 0..60 {
            tick(&mut b, METER_STEP_MS);
            if let Some(s) = b.stream_at(playing) {
                seen.push(percent_of(s.peak_level));
            }
        }
        seen.sort_unstable();
        seen.dedup();
        assert!(
            seen.len() > 5,
            "a playing meter took only {} distinct values in 60 steps: {seen:?}",
            seen.len()
        );
    }

    #[test]
    fn two_mixers_draw_two_different_meter_runs_and_one_seed_repeats_itself() {
        let run = |a: &mut MixerApp| {
            let mut v = Vec::new();
            for _ in 0..40 {
                tick(a, METER_STEP_MS);
                v.extend(
                    (0..a.stream_count())
                        .filter_map(|i| a.stream_at(i).map(|s| percent_of(s.peak_level))),
                );
            }
            v
        };
        let (mut a, mut b) = (app(), app());
        assert_eq!(run(&mut a), run(&mut b), "the same mixer ran two ways");

        let mut c = app();
        c.rng = SeededRng::new(0x1234_5678_9ABC_DEF0);
        assert_ne!(
            run(&mut c),
            run(&mut app()),
            "two seeds drew the same meter run"
        );
    }

    // ── The picker ─────────────────────────────────────────────────────────

    #[test]
    fn the_picker_opens_on_the_device_that_is_already_chosen() {
        for (open, pick) in [
            (Action::OpenOutput, Action::ChooseDevice),
            (Action::OpenInput, Action::ChooseDevice),
        ] {
            let mut a = app();
            a.apply(open);
            a.apply(Action::MovePickerRow(2));
            let row = a.picker_row();
            a.apply(pick);
            a.apply(open);
            assert_eq!(
                a.picker_row(),
                row,
                "{open:?} reopened on a row other than the chosen one"
            );
        }
    }

    #[test]
    fn every_row_the_picker_lists_can_be_chosen_with_the_pointer() {
        type Case = (Action, fn(usize) -> Target, fn(&MixerApp) -> usize);
        let cases: [Case; 2] = [
            (Action::OpenOutput, Target::OutputRow, |a| {
                a.selected_output()
            }),
            (Action::OpenInput, Target::InputRow, |a| a.selected_input()),
        ];
        for (open, row_target, chosen) in cases {
            let mut probe = app();
            probe.apply(open);
            let len = probe.picker_len();
            assert!(len > 1, "{open:?} lists {len} device(s) to choose between");
            for i in 0..len {
                let mut a = app();
                a.apply(open);
                let f = a.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
                let (x, y) = where_is(&f, row_target(i))
                    .unwrap_or_else(|| panic!("row {i} of {open:?} records no hit box"));
                assert_eq!(click(&mut a, x, y), EventResult::Consumed);
                assert_eq!(chosen(&a), i, "clicking row {i} chose another device");
                assert_eq!(a.picker(), Picker::None, "choosing left the sheet up");
            }
        }
    }

    #[test]
    fn the_picker_selection_moves_one_row_and_stops_at_the_ends() {
        let mut a = app();
        a.apply(Action::OpenOutput);
        let last = a.picker_len().saturating_sub(1);
        for _ in 0..last.saturating_add(3) {
            press(&mut a, Key::Down);
        }
        assert_eq!(a.picker_row(), last, "the picker ran off the bottom");
        for _ in 0..last.saturating_add(3) {
            press(&mut a, Key::Up);
        }
        assert_eq!(a.picker_row(), 0, "the picker ran off the top");
        press(&mut a, Key::Down);
        assert_eq!(a.picker_row(), 1, "Down moved more than one row");
    }

    #[test]
    fn enter_uses_the_row_the_picker_is_on_and_escape_leaves_it_alone() {
        let mut a = app();
        a.apply(Action::OpenOutput);
        press(&mut a, Key::Down);
        press(&mut a, Key::Enter);
        assert_eq!(a.selected_output(), 1, "Enter did not use the chosen row");
        assert_eq!(a.picker(), Picker::None, "Enter left the sheet up");

        let mut b = app();
        b.apply(Action::OpenOutput);
        press(&mut b, Key::Down);
        press(&mut b, Key::Escape);
        assert_eq!(b.selected_output(), 0, "Escape chose a device anyway");
        assert_eq!(b.picker(), Picker::None, "Escape left the sheet up");
    }

    #[test]
    fn a_click_anywhere_off_the_sheet_cancels_it() {
        // The backdrop is a hit box over the whole window recorded before the
        // sheet's own rows, so this is the whole of the modal behaviour.
        let mut a = app();
        a.apply(Action::OpenOutput);
        let l = a.layout();
        let outside = [
            (2.0, 2.0),
            (l.window.right() - 2.0, 2.0),
            (l.sheet.centre().0, l.sheet.y - 6.0),
        ];
        for (x, y) in outside {
            let mut b = app();
            b.apply(Action::OpenOutput);
            assert_eq!(click(&mut b, x, y), EventResult::Consumed);
            assert_eq!(
                b.picker(),
                Picker::None,
                "a click at ({x}, {y}) did not cancel the sheet"
            );
            assert_eq!(b.selected_output(), 0, "cancelling chose a device");
        }
    }

    #[test]
    fn while_the_sheet_is_up_no_click_reaches_the_controls_beneath_it() {
        let mut a = app();
        let f = a.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let (mx, my) = where_is(&f, Target::StreamMute(0)).expect("a mute button");
        let (fx, fy) = where_is(&f, Target::MasterFader).expect("the master fader");
        a.apply(Action::OpenOutput);
        let before = columns(&a);
        for (x, y) in [(mx, my), (fx, fy)] {
            let mut b = app();
            b.apply(Action::OpenOutput);
            click(&mut b, x, y);
            assert_eq!(
                columns(&b),
                before,
                "a click at ({x}, {y}) reached a control under the sheet"
            );
        }
    }

    #[test]
    fn while_the_sheet_is_up_the_mixer_keys_do_nothing() {
        // Not through the pointer: the sheet's block on the keyboard is a
        // separate piece of code from its block on the pointer, and a test that
        // only clicks cannot see it.
        for key in [Key::Left, Key::Right, Key::Tab, Key::M, Key::O, Key::I] {
            let mut a = app();
            a.apply(Action::OpenOutput);
            let before = (columns(&a), a.selection(), a.picker());
            let r = press(&mut a, key);
            assert_eq!(
                r,
                EventResult::Ignored,
                "{key:?} was answered under a sheet"
            );
            assert_eq!(
                (columns(&a), a.selection(), a.picker()),
                before,
                "{key:?} reached the mixer under a sheet"
            );
        }
    }

    #[test]
    fn up_and_down_move_the_picker_and_not_a_fader_while_the_sheet_is_up() {
        // The two views share Up and Down, so this is the one pair of keys that
        // could plausibly do both things at once.
        let mut a = app();
        a.apply(Action::OpenOutput);
        let before = columns(&a);
        press(&mut a, Key::Down);
        assert_eq!(a.picker_row(), 1);
        assert_eq!(
            columns(&a),
            before,
            "Down moved a fader as well as the sheet"
        );
    }

    // ── The arithmetic and the ordering ────────────────────────────────────

    #[test]
    fn a_level_is_a_percentage_of_itself_and_never_out_of_range() {
        for (level, want) in [
            (0.0_f32, 0_u8),
            (0.005, 1),
            (0.5, 50),
            (0.755, 76),
            (1.0, 100),
            (-3.0, 0),
            (17.0, 100),
            (f32::NAN, 0),
        ] {
            assert_eq!(percent_of(level), want, "percent_of({level})");
        }
    }

    #[test]
    fn decibels_and_linear_volume_are_inverses_where_both_are_defined() {
        for v in [0.05_f32, 0.1, 0.25, 0.5, 0.75, 1.0] {
            let round_trip = db_to_linear(linear_to_db(v));
            assert!(
                (round_trip - v).abs() < 0.001,
                "{v} went to {} dB and came back {round_trip}",
                linear_to_db(v)
            );
        }
        assert_eq!(linear_to_db(0.0), f32::NEG_INFINITY, "silence is not a dB");
        assert_eq!(linear_to_db(-1.0), f32::NEG_INFINITY);
        assert_eq!(db_to_linear(-80.0), 0.0, "-80dB is silence");
        assert_eq!(db_to_linear(-100.0), 0.0);
        assert_eq!(db_to_linear(0.0), 1.0, "0dB is full scale");
        assert!(
            db_to_linear(12.0) <= 1.0,
            "a positive dB escaped the range a volume can be in"
        );
        // Half the linear scale is about -6dB, which is the one figure anyone
        // reading a mixer knows by heart.
        assert!((linear_to_db(0.5) + 6.02).abs() < 0.05);
    }

    #[test]
    fn a_volume_reads_the_same_as_a_percentage_and_as_decibels() {
        assert_eq!(format_volume_percent(0.75), "75%");
        assert_eq!(format_volume_percent(0.0), "0%");
        assert_eq!(format_volume_percent(1.0), "100%");
        assert_eq!(format_volume_db(0.0), "-inf dB", "silence has no decibels");
        assert_eq!(format_volume_db(1.0), "0.0 dB");
        assert!(
            format_volume_db(0.5).starts_with("-6.0"),
            "half scale read {}",
            format_volume_db(0.5)
        );
    }

    #[test]
    fn a_stream_is_heard_at_its_own_fader_through_the_master() {
        let mut s = AudioStream::new(1, "Test", 0.5);
        assert_eq!(s.effective_volume(), 0.5);
        assert_eq!(s.volume_percent(), 50);
        s.toggle_mute();
        assert_eq!(s.effective_volume(), 0.0, "a muted stream is not heard");
        assert_eq!(
            s.volume, 0.5,
            "muting forgot where the fader was, so unmuting cannot put it back"
        );
        s.toggle_mute();
        assert_eq!(s.effective_volume(), 0.5);
        s.set_volume(4.0);
        assert_eq!(
            s.volume, 1.0,
            "a volume above full scale was taken at its word"
        );
        s.set_volume(-1.0);
        assert_eq!(s.volume, 0.0);
        assert_eq!(AudioStream::new(2, "Test", 9.0).volume, 1.0);

        assert!((combined_volume(0.5, 0.5) - 0.25).abs() < 0.0001);
        assert_eq!(combined_volume(1.0, 0.0), 0.0);
        assert_eq!(
            combined_volume(2.0, 2.0),
            1.0,
            "combined volume escaped the range"
        );
    }

    #[test]
    fn a_muted_master_silences_everything_without_moving_a_fader() {
        let mut a = app();
        let before = a.master_volume();
        a.apply(Action::ToggleMute(Selection::Master));
        assert_eq!(a.master_effective_volume(), 0.0);
        assert_eq!(
            a.master_volume(),
            before,
            "muting the master moved its fader"
        );
        a.apply(Action::ToggleMute(Selection::Master));
        assert_eq!(a.master_effective_volume(), before);
    }

    #[test]
    fn the_columns_are_playing_streams_first_and_then_by_name() {
        let a = app();
        let order: Vec<(bool, &str)> = a
            .sorted_streams()
            .iter()
            .map(|s| (s.playing, s.app_name.as_str()))
            .collect();
        let mut want = order.clone();
        want.sort_by(|x, y| y.0.cmp(&x.0).then_with(|| x.1.cmp(y.1)));
        assert_eq!(order, want, "the columns are not in the order they claim");
        assert_eq!(
            a.order().len(),
            a.stream_count(),
            "a stream lost its column"
        );
        let mut ids = a.order();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), a.stream_count(), "two columns share one stream");
    }

    #[test]
    fn a_column_index_means_the_same_stream_to_the_screen_and_to_the_keyboard() {
        // `stream_at` and the drawing pass both go through `order()`, so what
        // this really pins is that the *name drawn* in column i is the name of
        // the stream `Selection::Stream(i)` resolves to.
        let a = app();
        let f = a.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let l = a.layout();
        for i in 0..a.stream_count() {
            let col = box_of(&f, Target::StreamColumn(i)).expect("a column");
            let name_box = l.name_of(col);
            let drawn = f
                .commands()
                .iter()
                .find_map(|c| match c {
                    // Every column's name sits in the same horizontal band, so
                    // the row alone finds the master's name for every column.
                    RenderCommand::Text { text, x, y, .. }
                        if *y >= name_box.y - 0.01
                            && *y < name_box.bottom()
                            && *x >= name_box.x - 0.01
                            && *x <= name_box.right() + 0.01 =>
                    {
                        Some(text.clone())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("column {i} has no name drawn on it"));
            assert_eq!(
                drawn,
                a.stream_at(i).expect("the stream").app_name,
                "column {i} is drawn with one stream's name and answers for another"
            );
        }
    }

    #[test]
    fn a_device_says_what_it_is() {
        let d = AudioDevice {
            id: 0,
            name: "Speakers".to_string(),
            device_type: DeviceType::Output,
            sample_rate: 48000,
            bit_depth: 24,
            channels: 2,
        };
        assert_eq!(d.properties(), "48000Hz / 24bit / 2ch");
    }

    // ── The window ─────────────────────────────────────────────────────────

    #[test]
    fn a_resize_is_the_only_thing_that_changes_the_size_the_layout_is_read_at() {
        let mut a = app();
        assert_eq!(a.size(), (WINDOW_WIDTH, WINDOW_HEIGHT));
        handle_event(
            &mut a,
            &Event::Resize {
                width: 400,
                height: 300,
            },
        );
        assert_eq!(a.size(), (400.0, 300.0));
        assert_eq!(a.layout().window, Rect::new(0.0, 0.0, 400.0, 300.0));
        // A window of no size is a divide by zero waiting to happen.
        a.resize(0.0, 0.0);
        assert_eq!(
            a.size(),
            (1.0, 1.0),
            "a zero-sized window was taken at its word"
        );
    }

    #[test]
    fn the_window_is_told_who_it_is() {
        let a = app();
        assert_eq!(a.app_id(), "mixer");
        assert!(!a.title().is_empty());
        assert_eq!(
            a.initial_size(),
            (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32),
            "the window opens at a size the layout was never designed for"
        );
    }

    #[test]
    fn the_window_gets_a_tree_at_the_size_it_asked_for_and_keeps_it() {
        let mut a = app();
        let tree = a.render(500.0, 420.0);
        assert_eq!(
            a.size(),
            (500.0, 420.0),
            "render did not read the live size"
        );
        assert!(
            !tree.commands.is_empty(),
            "the window was handed an empty picture"
        );
    }

    #[test]
    fn an_event_the_program_does_not_answer_is_left_alone() {
        let mut a = app();
        let before = snap(&a);
        assert_eq!(
            handle_event(&mut a, &Event::CloseRequested),
            EventResult::Ignored,
            "a close request was swallowed, so the window can never be closed"
        );
        assert_eq!(snap(&a), before);
    }

    #[test]
    fn the_probe_and_the_window_drive_the_same_body() {
        use guitk::probe;
        let mut a = app();
        probe::click(&mut a, Target::StreamMute(0));
        assert_eq!(
            a.muted_of(Selection::Stream(0)),
            Some(true),
            "a probe click did not reach the handler the window uses"
        );
        assert_eq!(a.selection(), Selection::Stream(0));

        let mut b = app();
        probe::key(&mut b, &key_ev(Key::Right, Modifiers::NONE, true));
        assert_eq!(b.selection(), Selection::Stream(0));
        assert_eq!(MixerApp::SIZE, (WINDOW_WIDTH, WINDOW_HEIGHT));
    }
}
