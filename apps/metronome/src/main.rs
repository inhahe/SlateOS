//! Slate OS Metronome
//!
//! A musical metronome with BPM control, time signature selection,
//! visual beat indicator, tap tempo, accent patterns, and subdivisions.
//!
//! The window, the connection and the event loop are `oswindow::app`'s; this
//! file supplies only what is actually a metronome's own — what to do with an
//! event, what to draw, and how often it needs the clock. See
//! `known-issues.md` → `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` for why that
//! division exists rather than a hand-written strap per app.
//!
//! There is deliberately no crate-wide `#![allow(dead_code)]` here. This file
//! carried one, and it is the lint that would have said the whole application
//! was unreachable from `main` — lesson 46 in `known-issues.md`: a blanket
//! allow disarms the one check that finds lesson 45.

#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::fn_params_excessive_bools)]

use std::process::ExitCode;
use std::time::Duration;

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use oswindow::app::{App, Response};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const COL_BASE: u32 = 0x1E1E2E;
const COL_MANTLE: u32 = 0x181825;
const COL_SURFACE0: u32 = 0x313244;
const COL_SURFACE1: u32 = 0x45475A;
const COL_TEXT: u32 = 0xCDD6F4;
const COL_SUBTEXT0: u32 = 0xA6ADC8;
const COL_GREEN: u32 = 0xA6E3A1;
const COL_RED: u32 = 0xF38BA8;
const COL_YELLOW: u32 = 0xF9E2AF;
const COL_LAVENDER: u32 = 0xB4BEFE;
const COL_OVERLAY0: u32 = 0x6C7086;
const COL_TEAL: u32 = 0x94E2D5;
const COL_MAUVE: u32 = 0xCBA6F7;

const MIN_BPM: u32 = 20;
const MAX_BPM: u32 = 300;
const TAP_HISTORY_SIZE: usize = 8;
/// How long a tap stays part of the current measurement.
///
/// Two seconds is 30 BPM, below `MIN_BPM`, so a gap this long is not a slow
/// tempo — it is somebody starting again. Without the rule, a tap made minutes
/// after the last one averages a 600-second "interval" into the tempo and pins
/// it to the floor.
///
/// It has a second job. The frame clock is only armed while something needs
/// advancing (see `MetronomeApp::tick_interval`), and a tap history that never
/// emptied would keep it armed for the life of the process — one tap, and a
/// stopped metronome holds the desktop awake for ever. Expiring the history is
/// what lets the clock stop, so the two properties are one rule rather than
/// two that must be kept in step.
const TAP_STALE_MS: u64 = 2_000;
/// How long the beat indicator stays lit, in milliseconds.
///
/// Named rather than written twice: `toggle_play` lights beat one and `tick`
/// lights every beat after it, and a flash that differs between the two would
/// read as a stutter on the downbeat.
const BEAT_FLASH_MS: u64 = 150;

// ---------------------------------------------------------------------------
// Time signature
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TimeSignature {
    beats_per_measure: u32,
    beat_value: u32, // 4 = quarter note, 8 = eighth note
}

impl TimeSignature {
    fn display(&self) -> String {
        format!("{}/{}", self.beats_per_measure, self.beat_value)
    }
}

const COMMON_SIGNATURES: &[TimeSignature] = &[
    TimeSignature {
        beats_per_measure: 2,
        beat_value: 4,
    },
    TimeSignature {
        beats_per_measure: 3,
        beat_value: 4,
    },
    TimeSignature {
        beats_per_measure: 4,
        beat_value: 4,
    },
    TimeSignature {
        beats_per_measure: 5,
        beat_value: 4,
    },
    TimeSignature {
        beats_per_measure: 6,
        beat_value: 8,
    },
    TimeSignature {
        beats_per_measure: 7,
        beat_value: 8,
    },
    TimeSignature {
        beats_per_measure: 3,
        beat_value: 8,
    },
    TimeSignature {
        beats_per_measure: 9,
        beat_value: 8,
    },
    TimeSignature {
        beats_per_measure: 12,
        beat_value: 8,
    },
];

// ---------------------------------------------------------------------------
// Subdivision
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Subdivision {
    None,
    Eighth,    // 2 per beat
    Triplet,   // 3 per beat
    Sixteenth, // 4 per beat
}

impl Subdivision {
    fn name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Eighth => "8th",
            Self::Triplet => "Triplet",
            Self::Sixteenth => "16th",
        }
    }

    fn subdivisions_per_beat(self) -> u32 {
        match self {
            Self::None => 1,
            Self::Eighth => 2,
            Self::Triplet => 3,
            Self::Sixteenth => 4,
        }
    }

    fn cycle(self) -> Self {
        match self {
            Self::None => Self::Eighth,
            Self::Eighth => Self::Triplet,
            Self::Triplet => Self::Sixteenth,
            Self::Sixteenth => Self::None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tempo marking
// ---------------------------------------------------------------------------

fn tempo_name(bpm: u32) -> &'static str {
    match bpm {
        0..=24 => "Larghissimo",
        25..=39 => "Grave",
        40..=54 => "Largo",
        55..=65 => "Larghetto",
        66..=75 => "Adagio",
        76..=107 => "Andante",
        108..=119 => "Moderato",
        120..=155 => "Allegro",
        156..=175 => "Vivace",
        176..=199 => "Presto",
        _ => "Prestissimo",
    }
}

// ---------------------------------------------------------------------------
// Main app
// ---------------------------------------------------------------------------

struct MetronomeApp {
    bpm: u32,
    time_signature: TimeSignature,
    sig_index: usize,
    subdivision: Subdivision,
    playing: bool,

    // Beat tracking
    current_beat: u32, // 0-indexed within measure
    current_sub: u32,  // 0-indexed within beat
    total_beats: u64,
    last_beat_time_ms: u64,
    beat_flash_ms: u64, // time remaining for beat flash visual

    /// The app's own monotonic clock, in milliseconds since it started.
    ///
    /// `Event::Tick` carries an *interval*, not a timestamp, so an app that
    /// needs to compare two moments has to accumulate one itself.  A
    /// metronome needs exactly that in two places: scheduling the next beat,
    /// and tap tempo, which is nothing but the gaps between taps.
    now_ms: u64,

    // Tap tempo
    tap_times_ms: Vec<u64>,

    // Accent pattern: true = accented, one per beat
    accents: Vec<bool>,

    // Practice mode
    practice_mode: bool,
    practice_start_bpm: u32,
    practice_target_bpm: u32,
    practice_increment: u32,
    practice_measures: u32,
    practice_measure_count: u32,

    // View
    show_settings: bool,
}

impl MetronomeApp {
    fn new() -> Self {
        let sig = COMMON_SIGNATURES[2]; // 4/4
        let mut accents = vec![false; sig.beats_per_measure as usize];
        if !accents.is_empty() {
            accents[0] = true;
        }
        Self {
            bpm: 120,
            time_signature: sig,
            sig_index: 2,
            subdivision: Subdivision::None,
            playing: false,
            current_beat: 0,
            current_sub: 0,
            total_beats: 0,
            last_beat_time_ms: 0,
            beat_flash_ms: 0,
            now_ms: 0,
            tap_times_ms: Vec::new(),
            accents,
            practice_mode: false,
            practice_start_bpm: 80,
            practice_target_bpm: 160,
            practice_increment: 10,
            practice_measures: 4,
            practice_measure_count: 0,
            show_settings: false,
        }
    }

    fn beat_interval_ms(&self) -> u64 {
        if self.bpm == 0 {
            return 1000;
        }
        let sub_div = self.subdivision.subdivisions_per_beat();
        60_000 / (self.bpm as u64 * sub_div as u64)
    }

    fn set_bpm(&mut self, bpm: u32) {
        self.bpm = bpm.clamp(MIN_BPM, MAX_BPM);
    }

    fn increase_bpm(&mut self, amount: u32) {
        self.set_bpm(self.bpm.saturating_add(amount));
    }

    fn decrease_bpm(&mut self, amount: u32) {
        self.set_bpm(self.bpm.saturating_sub(amount));
    }

    fn set_time_signature(&mut self, idx: usize) {
        if idx < COMMON_SIGNATURES.len() {
            self.sig_index = idx;
            self.time_signature = COMMON_SIGNATURES[idx];
            self.accents = vec![false; self.time_signature.beats_per_measure as usize];
            if !self.accents.is_empty() {
                self.accents[0] = true;
            }
            self.current_beat = 0;
            self.current_sub = 0;
        }
    }

    fn cycle_time_signature(&mut self) {
        let next = (self.sig_index + 1) % COMMON_SIGNATURES.len();
        self.set_time_signature(next);
    }

    fn toggle_accent(&mut self, beat: usize) {
        if beat < self.accents.len() {
            self.accents[beat] = !self.accents[beat];
        }
    }

    fn tap_tempo(&mut self, time_ms: u64) {
        self.tap_times_ms.push(time_ms);
        if self.tap_times_ms.len() > TAP_HISTORY_SIZE {
            self.tap_times_ms.remove(0);
        }

        if self.tap_times_ms.len() >= 2 {
            let intervals: Vec<u64> = self
                .tap_times_ms
                .windows(2)
                .map(|w| w[1].saturating_sub(w[0]))
                .collect();
            let avg_interval: u64 = intervals.iter().sum::<u64>() / intervals.len() as u64;
            if let Some(calculated_bpm) = 60_000u64.checked_div(avg_interval) {
                self.set_bpm(calculated_bpm as u32);
            }
        }
    }

    /// Drop the tap history once the last tap is older than [`TAP_STALE_MS`].
    ///
    /// Called from `tick` rather than from `tap_tempo` so that one rule does
    /// both jobs: the next tap after a long pause starts a fresh measurement
    /// *because* the history has already been emptied, and the emptying is
    /// what lets `tick_interval` give the clock back. Putting the test in
    /// `tap_tempo` instead would fix the tempo and leave the clock running.
    fn forget_stale_taps(&mut self) {
        if let Some(&last) = self.tap_times_ms.last()
            && self.now_ms.saturating_sub(last) > TAP_STALE_MS
        {
            self.tap_times_ms.clear();
        }
    }

    fn clear_tap(&mut self) {
        self.tap_times_ms.clear();
    }

    fn toggle_play(&mut self) {
        self.playing = !self.playing;
        if self.playing {
            self.current_beat = 0;
            self.current_sub = 0;
            self.total_beats = 0;
            // Beat one lands on the keypress, not one interval after it: a
            // metronome you start on the downbeat is the point of starting
            // it there.  The flash is set here for the same reason -- the
            // first beat is displayed by `toggle_play`, and `tick` takes
            // over from the second.
            self.last_beat_time_ms = self.now_ms;
            self.beat_flash_ms = BEAT_FLASH_MS;
            if self.practice_mode {
                self.bpm = self.practice_start_bpm;
                self.practice_measure_count = 0;
            }
        }
    }

    /// Advance the metronome by `delta_ms`, the interval since the last tick.
    ///
    /// An interval, not a timestamp: that is what [`Event::Tick`] carries
    /// (`oswindow` computes `now - this window's previous tick`), and every
    /// other tick consumer in the tree reads it that way.  This used to take
    /// an absolute `current_ms`, which nothing could have supplied, because
    /// nothing called it at all -- see known-issues.md lesson 45.  The old
    /// body also decayed the beat flash by a hard-coded `16`, a guess at the
    /// frame interval; the real one now arrives with the event.
    ///
    /// Returns whether anything the user can *see* changed. Most ticks change
    /// nothing — at 120 BPM and 60 fps, 29 of every 30 — and a frame per tick
    /// would spend a desktop's whole budget redrawing a display that reads the
    /// same. The verdict is computed here rather than by comparing fields from
    /// outside because this is where the mutations are: a new visible effect
    /// added to this function has its answer three lines away.
    fn tick(&mut self, delta_ms: u64) -> bool {
        self.now_ms = self.now_ms.saturating_add(delta_ms);
        let was_flashing = self.beat_flash_ms > 0;
        self.beat_flash_ms = self.beat_flash_ms.saturating_sub(delta_ms);
        // The flash is drawn as lit-or-not (`beat_flash_ms > 0`), so only the
        // crossing to zero is a visible change, not the countdown itself.
        let unlit = was_flashing && self.beat_flash_ms == 0;

        // Nothing anyone can see, but it is what allows the clock to stop.
        self.forget_stale_taps();

        if !self.playing {
            return unlit;
        }

        let interval = self.beat_interval_ms();
        if self.now_ms.saturating_sub(self.last_beat_time_ms) < interval {
            return unlit;
        }

        // Advance the beat clock by exactly one interval rather than snapping
        // it to now.  The tick that crosses a beat boundary is up to a frame
        // late, and snapping would fold that lateness into every beat: at
        // 60 fps and 120 BPM that is ~16 ms on a 500 ms beat, so the
        // metronome would run about 3% slow and drift against anything it
        // was played along with.  Advancing by the interval keeps the phase.
        self.last_beat_time_ms = self.last_beat_time_ms.saturating_add(interval);
        // Unless we are still a whole beat behind -- the window went
        // unticked, or the tempo just jumped -- in which case resync to now
        // rather than fire a burst of catch-up beats at the user.
        if self.now_ms.saturating_sub(self.last_beat_time_ms) >= interval {
            self.last_beat_time_ms = self.now_ms;
        }

        self.advance_beat();
        self.beat_flash_ms = BEAT_FLASH_MS;
        true
    }

    fn advance_beat(&mut self) {
        let subs = self.subdivision.subdivisions_per_beat();
        self.current_sub += 1;
        if self.current_sub >= subs {
            self.current_sub = 0;
            self.current_beat += 1;
            self.total_beats += 1;

            if self.current_beat >= self.time_signature.beats_per_measure {
                self.current_beat = 0;
                // Practice mode: increment BPM after N measures
                if self.practice_mode {
                    self.practice_measure_count += 1;
                    if self.practice_measure_count >= self.practice_measures
                        && self.bpm < self.practice_target_bpm
                    {
                        self.practice_measure_count = 0;
                        self.increase_bpm(self.practice_increment);
                        if self.bpm > self.practice_target_bpm {
                            self.bpm = self.practice_target_bpm;
                        }
                    }
                }
            }
        }
    }

    fn handle_key(&mut self, event: &KeyEvent) {
        if !event.pressed {
            return;
        }

        if self.show_settings {
            self.handle_settings(event);
            return;
        }

        match event.key {
            Key::Space => self.toggle_play(),
            Key::Up => self.increase_bpm(if event.modifiers.shift { 10 } else { 1 }),
            Key::Down => self.decrease_bpm(if event.modifiers.shift { 10 } else { 1 }),
            Key::T => {
                // Tap tempo needs a clock, and this arm was empty for want of
                // one -- its comment said "in a real app this would use
                // system time".  `now_ms` is that clock now, accumulated from
                // the tick intervals, which is all tap tempo ever needed:
                // it reads only the *gaps* between taps, so an origin of
                // "when the app started" serves as well as a wall clock.
                self.tap_tempo(self.now_ms);
            }
            Key::Backspace => {
                // Clearing the tap history is the way out of a mistimed tap;
                // without it a stray tap poisons the average until the ring
                // buffer rolls it off.
                self.clear_tap();
            }
            Key::S => {
                self.subdivision = self.subdivision.cycle();
            }
            Key::G => {
                self.cycle_time_signature();
            }
            Key::P => {
                self.practice_mode = !self.practice_mode;
            }
            Key::Enter => {
                self.show_settings = !self.show_settings;
            }
            Key::R => {
                self.playing = false;
                self.current_beat = 0;
                self.current_sub = 0;
                self.total_beats = 0;
                self.practice_measure_count = 0;
                self.beat_flash_ms = 0;
            }
            Key::Num1
            | Key::Num2
            | Key::Num3
            | Key::Num4
            | Key::Num5
            | Key::Num6
            | Key::Num7
            | Key::Num8
            | Key::Num9 => {
                let beat_num = match event.key {
                    Key::Num1 => 0,
                    Key::Num2 => 1,
                    Key::Num3 => 2,
                    Key::Num4 => 3,
                    Key::Num5 => 4,
                    Key::Num6 => 5,
                    Key::Num7 => 6,
                    Key::Num8 => 7,
                    Key::Num9 => 8,
                    _ => 0,
                };
                self.toggle_accent(beat_num);
            }
            _ => {}
        }
    }

    fn handle_settings(&mut self, event: &KeyEvent) {
        match event.key {
            Key::Escape | Key::Enter => {
                self.show_settings = false;
            }
            Key::Up if self.practice_mode => {
                self.practice_target_bpm = (self.practice_target_bpm + 10).min(MAX_BPM);
            }
            Key::Down if self.practice_mode => {
                self.practice_target_bpm = self.practice_target_bpm.saturating_sub(10).max(MIN_BPM);
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    /// The drawing itself, as a flat command list.
    ///
    /// Kept separate from `App::render` — which wraps this in a `RenderTree` —
    /// because the tests assert over the commands, and because an inherent
    /// `render` alongside the trait's would silently win the method lookup:
    /// every existing `app.render(600.0, 800.0)` would keep compiling while
    /// testing the wrong function.
    fn render_commands(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: Color::from_hex(COL_BASE),
            corner_radii: CornerRadii::ZERO,
        });

        if self.show_settings {
            self.render_settings(&mut cmds, width);
        } else {
            self.render_main(&mut cmds, width);
        }

        cmds
    }

    fn render_main(&self, cmds: &mut Vec<RenderCommand>, _width: f32) {
        // Title
        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 15.0,
            text: String::from("Metronome"),
            color: Color::from_hex(COL_LAVENDER),
            font_size: 28.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Playing indicator
        let (status_text, status_color) = if self.playing {
            ("● PLAYING", COL_GREEN)
        } else {
            ("○ STOPPED", COL_OVERLAY0)
        };
        cmds.push(RenderCommand::Text {
            x: 250.0,
            y: 22.0,
            text: String::from(status_text),
            color: Color::from_hex(status_color),
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // BPM display
        cmds.push(RenderCommand::FillRect {
            x: 30.0,
            y: 55.0,
            width: 250.0,
            height: 90.0,
            color: Color::from_hex(COL_MANTLE),
            corner_radii: CornerRadii::all(12.0),
        });
        cmds.push(RenderCommand::Text {
            x: 60.0,
            y: 65.0,
            text: self.bpm.to_string(),
            color: Color::from_hex(COL_TEXT),
            font_size: 56.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cmds.push(RenderCommand::Text {
            x: 200.0,
            y: 95.0,
            text: String::from("BPM"),
            color: Color::from_hex(COL_SUBTEXT0),
            font_size: 18.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Tempo name
        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 150.0,
            text: String::from(tempo_name(self.bpm)),
            color: Color::from_hex(COL_MAUVE),
            font_size: 18.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Time signature & subdivision
        let info_y = 175.0;
        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: info_y,
            text: format!(
                "Time: {}  |  Sub: {}  |  Interval: {}ms",
                self.time_signature.display(),
                self.subdivision.name(),
                self.beat_interval_ms()
            ),
            color: Color::from_hex(COL_SUBTEXT0),
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Beat indicator (circles for each beat in the measure)
        let beat_y = 215.0;
        let beats = self.time_signature.beats_per_measure;
        let circle_size = 36.0_f32.min(400.0 / beats as f32 - 8.0);
        let _total_w = beats as f32 * (circle_size + 8.0) - 8.0;
        let start_x = 30.0;

        for i in 0..beats {
            let cx = start_x + i as f32 * (circle_size + 8.0);
            let is_current = self.playing && i == self.current_beat && self.current_sub == 0;
            let is_accented = (i as usize) < self.accents.len() && self.accents[i as usize];

            let color = if is_current && self.beat_flash_ms > 0 {
                if is_accented {
                    Color::from_hex(COL_RED)
                } else {
                    Color::from_hex(COL_GREEN)
                }
            } else if is_accented {
                Color::from_hex(COL_SURFACE1)
            } else {
                Color::from_hex(COL_SURFACE0)
            };

            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: beat_y,
                width: circle_size,
                height: circle_size,
                color,
                corner_radii: CornerRadii::all(circle_size / 2.0),
            });

            // Beat number
            cmds.push(RenderCommand::Text {
                x: cx + circle_size / 2.0 - 5.0,
                y: beat_y + circle_size / 2.0 - 8.0,
                text: (i + 1).to_string(),
                color: if is_current && self.beat_flash_ms > 0 {
                    Color::from_hex(COL_BASE)
                } else {
                    Color::from_hex(COL_TEXT)
                },
                font_size: 16.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Subdivision indicators
        if self.subdivision != Subdivision::None && self.playing {
            let sub_y = beat_y + circle_size + 10.0;
            let subs = self.subdivision.subdivisions_per_beat();
            for s in 0..subs {
                let sx = start_x + s as f32 * 14.0;
                let is_current_sub = s == self.current_sub;
                cmds.push(RenderCommand::FillRect {
                    x: sx,
                    y: sub_y,
                    width: 10.0,
                    height: 10.0,
                    color: if is_current_sub && self.beat_flash_ms > 0 {
                        Color::from_hex(COL_TEAL)
                    } else {
                        Color::from_hex(COL_SURFACE0)
                    },
                    corner_radii: CornerRadii::all(5.0),
                });
            }
        }

        // Stats
        let stats_y = beat_y + circle_size + 40.0;
        if self.playing {
            let measure = self.total_beats / self.time_signature.beats_per_measure as u64 + 1;
            cmds.push(RenderCommand::Text {
                x: 30.0,
                y: stats_y,
                text: format!(
                    "Beat: {}/{}  |  Measure: {}  |  Total beats: {}",
                    self.current_beat + 1,
                    self.time_signature.beats_per_measure,
                    measure,
                    self.total_beats
                ),
                color: Color::from_hex(COL_TEAL),
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Practice mode indicator
        if self.practice_mode {
            cmds.push(RenderCommand::FillRect {
                x: 30.0,
                y: stats_y + 25.0,
                width: 400.0,
                height: 30.0,
                color: Color::from_hex(COL_SURFACE0),
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: 40.0,
                y: stats_y + 30.0,
                text: format!(
                    "Practice: {} → {} BPM (+{} every {} measures)",
                    self.practice_start_bpm,
                    self.practice_target_bpm,
                    self.practice_increment,
                    self.practice_measures
                ),
                color: Color::from_hex(COL_YELLOW),
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Controls
        let ctrl_y = stats_y + 65.0;
        let controls = [
            "Space: Play/Stop",
            "↑/↓: BPM ±1 (Shift: ±10)",
            "S: Subdivision  |  G: Time Sig",
            "1-9: Toggle accent  |  P: Practice",
            "T: Tap tempo  |  Backspace: Clear taps",
            "R: Reset  |  Enter: Settings",
        ];
        for (i, line) in controls.iter().enumerate() {
            cmds.push(RenderCommand::Text {
                x: 30.0,
                y: ctrl_y + i as f32 * 18.0,
                text: String::from(*line),
                color: Color::from_hex(COL_OVERLAY0),
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }

    fn render_settings(&self, cmds: &mut Vec<RenderCommand>, _width: f32) {
        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 20.0,
            text: String::from("Metronome Settings"),
            color: Color::from_hex(COL_LAVENDER),
            font_size: 24.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 55.0,
            text: String::from("Esc/Enter: Back"),
            color: Color::from_hex(COL_OVERLAY0),
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        let settings = [
            (format!("BPM: {}", self.bpm), COL_TEXT),
            (
                format!("Time Signature: {}", self.time_signature.display()),
                COL_TEXT,
            ),
            (
                format!("Subdivision: {}", self.subdivision.name()),
                COL_TEXT,
            ),
            (format!("Tempo: {}", tempo_name(self.bpm)), COL_MAUVE),
            (
                format!(
                    "Practice Mode: {}",
                    if self.practice_mode { "ON" } else { "OFF" }
                ),
                COL_YELLOW,
            ),
            (
                format!(
                    "Practice Target: {} BPM (↑/↓ to adjust)",
                    self.practice_target_bpm
                ),
                COL_TEAL,
            ),
            (
                format!("Practice Increment: +{} BPM", self.practice_increment),
                COL_TEAL,
            ),
            (
                format!("Practice Measures: {}", self.practice_measures),
                COL_TEAL,
            ),
        ];

        for (i, (text, col)) in settings.iter().enumerate() {
            cmds.push(RenderCommand::FillRect {
                x: 30.0,
                y: 80.0 + i as f32 * 38.0,
                width: 450.0,
                height: 32.0,
                color: Color::from_hex(COL_SURFACE0),
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: 45.0,
                y: 86.0 + i as f32 * 38.0,
                text: text.clone(),
                color: Color::from_hex(*col),
                font_size: 15.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }
}

impl App for MetronomeApp {
    fn title(&self) -> String {
        String::from("Metronome")
    }

    fn initial_size(&self) -> (u32, u32) {
        (520, 720)
    }

    /// The clock is asked for only while something is actually moving.
    ///
    /// This is the method the harness's docs single out, and the metronome is
    /// the case that shows why it is `Option` rather than a constant: a stopped
    /// metronome has nothing to advance, and an app that keeps asking for ticks
    /// with nothing to advance holds the whole desktop awake — the compositor
    /// cannot park while any window has a deadline armed.
    ///
    /// Three things need it, and each stops needing it on its own:
    ///
    /// * `playing` — the beat itself.
    /// * `beat_flash_ms` — the indicator is still lit and must go out. Without
    ///   this the flash from the last beat before Stop would stay on screen,
    ///   because the tick that would have cleared it is the one we declined.
    /// * a live tap history — `now_ms` is the only clock tap tempo has, so it
    ///   must keep running between taps or every tap would read as
    ///   simultaneous. [`TAP_STALE_MS`] is what makes this term expire.
    ///
    /// 16 ms is a floor, not a promise: the harness may deliver late and the
    /// app must advance by the `elapsed_ms` it is given, never by this value.
    fn tick_interval(&self) -> Option<Duration> {
        if self.playing || self.beat_flash_ms > 0 || !self.tap_times_ms.is_empty() {
            Some(Duration::from_millis(16))
        } else {
            None
        }
    }

    fn on_event(&mut self, event: &Event) -> Response {
        match event {
            // Redraw unconditionally on a key, without working out whether
            // this particular key changed anything. A keystroke is one event
            // at human speed, so an occasional wasted frame costs nothing; a
            // tick is 60 a second, which is why that arm below does the work
            // to answer honestly.
            Event::Key(ke) => {
                self.handle_key(ke);
                Response::Redraw
            }
            // Without this the metronome never beat: `tick` was correct and
            // tested, and nothing called it. known-issues.md lesson 45, and
            // lesson 47 for the shape it takes in a GUI app — the window still
            // laid out, still repainted and still answered the keyboard while
            // showing a beat counter frozen at one.
            Event::Tick { elapsed_ms } => {
                if self.tick(*elapsed_ms) {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
            // `Resize` and `ScaleChanged` are absent on purpose: the harness
            // redraws for those itself, because the frame on screen was drawn
            // at the old geometry whatever the app thinks. See
            // `oswindow::app::drive`.
            _ => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        RenderTree {
            commands: self.render_commands(width, height),
        }
    }
}

fn main() -> ExitCode {
    oswindow::app::launch("metronome", &mut MetronomeApp::new())
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use guitk::event::Modifiers;

    use super::*;

    fn make_key(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    fn make_shift_key(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::shift(),
            text: String::new(),
        }
    }

    // --- Time signature ---

    #[test]
    fn time_signature_display() {
        let ts = TimeSignature {
            beats_per_measure: 4,
            beat_value: 4,
        };
        assert_eq!(ts.display(), "4/4");
        let ts2 = TimeSignature {
            beats_per_measure: 6,
            beat_value: 8,
        };
        assert_eq!(ts2.display(), "6/8");
    }

    #[test]
    fn common_signatures_count() {
        assert_eq!(COMMON_SIGNATURES.len(), 9);
    }

    // --- Subdivision ---

    #[test]
    fn subdivision_names() {
        assert_eq!(Subdivision::None.name(), "None");
        assert_eq!(Subdivision::Eighth.name(), "8th");
        assert_eq!(Subdivision::Triplet.name(), "Triplet");
        assert_eq!(Subdivision::Sixteenth.name(), "16th");
    }

    #[test]
    fn subdivision_counts() {
        assert_eq!(Subdivision::None.subdivisions_per_beat(), 1);
        assert_eq!(Subdivision::Eighth.subdivisions_per_beat(), 2);
        assert_eq!(Subdivision::Triplet.subdivisions_per_beat(), 3);
        assert_eq!(Subdivision::Sixteenth.subdivisions_per_beat(), 4);
    }

    #[test]
    fn subdivision_cycle() {
        let s = Subdivision::None;
        let s = s.cycle();
        assert_eq!(s, Subdivision::Eighth);
        let s = s.cycle();
        assert_eq!(s, Subdivision::Triplet);
        let s = s.cycle();
        assert_eq!(s, Subdivision::Sixteenth);
        let s = s.cycle();
        assert_eq!(s, Subdivision::None);
    }

    // --- Tempo names ---

    #[test]
    fn tempo_name_ranges() {
        assert_eq!(tempo_name(20), "Larghissimo");
        assert_eq!(tempo_name(30), "Grave");
        assert_eq!(tempo_name(50), "Largo");
        assert_eq!(tempo_name(60), "Larghetto");
        assert_eq!(tempo_name(70), "Adagio");
        assert_eq!(tempo_name(90), "Andante");
        assert_eq!(tempo_name(110), "Moderato");
        assert_eq!(tempo_name(120), "Allegro");
        assert_eq!(tempo_name(160), "Vivace");
        assert_eq!(tempo_name(180), "Presto");
        assert_eq!(tempo_name(210), "Prestissimo");
    }

    // --- App creation ---

    #[test]
    fn new_app() {
        let app = MetronomeApp::new();
        assert_eq!(app.bpm, 120);
        assert_eq!(app.time_signature.beats_per_measure, 4);
        assert_eq!(app.time_signature.beat_value, 4);
        assert!(!app.playing);
        assert_eq!(app.current_beat, 0);
        assert_eq!(app.subdivision, Subdivision::None);
    }

    #[test]
    fn default_accents() {
        let app = MetronomeApp::new();
        assert_eq!(app.accents.len(), 4);
        assert!(app.accents[0]); // first beat accented
        assert!(!app.accents[1]);
        assert!(!app.accents[2]);
        assert!(!app.accents[3]);
    }

    // --- BPM control ---

    #[test]
    fn set_bpm() {
        let mut app = MetronomeApp::new();
        app.set_bpm(100);
        assert_eq!(app.bpm, 100);
    }

    #[test]
    fn set_bpm_clamped_low() {
        let mut app = MetronomeApp::new();
        app.set_bpm(5);
        assert_eq!(app.bpm, MIN_BPM);
    }

    #[test]
    fn set_bpm_clamped_high() {
        let mut app = MetronomeApp::new();
        app.set_bpm(500);
        assert_eq!(app.bpm, MAX_BPM);
    }

    #[test]
    fn increase_bpm() {
        let mut app = MetronomeApp::new();
        app.increase_bpm(5);
        assert_eq!(app.bpm, 125);
    }

    #[test]
    fn decrease_bpm() {
        let mut app = MetronomeApp::new();
        app.decrease_bpm(10);
        assert_eq!(app.bpm, 110);
    }

    #[test]
    fn increase_bpm_capped() {
        let mut app = MetronomeApp::new();
        app.bpm = 298;
        app.increase_bpm(5);
        assert_eq!(app.bpm, MAX_BPM);
    }

    // --- Beat interval ---

    #[test]
    fn beat_interval_120bpm() {
        let app = MetronomeApp::new();
        assert_eq!(app.beat_interval_ms(), 500); // 60000/120
    }

    #[test]
    fn beat_interval_with_subdivision() {
        let mut app = MetronomeApp::new();
        app.subdivision = Subdivision::Eighth;
        // 60000 / (120 * 2) = 250
        assert_eq!(app.beat_interval_ms(), 250);
    }

    #[test]
    fn beat_interval_triplet() {
        let mut app = MetronomeApp::new();
        app.subdivision = Subdivision::Triplet;
        // 60000 / (120 * 3) = 166
        assert_eq!(app.beat_interval_ms(), 166);
    }

    // --- Time signature switching ---

    #[test]
    fn cycle_time_signature() {
        let mut app = MetronomeApp::new();
        assert_eq!(app.time_signature.beats_per_measure, 4);
        app.cycle_time_signature();
        assert_eq!(app.time_signature.beats_per_measure, 5);
    }

    #[test]
    fn cycle_time_signature_wraps() {
        let mut app = MetronomeApp::new();
        for _ in 0..COMMON_SIGNATURES.len() {
            app.cycle_time_signature();
        }
        // Should wrap back to first
        assert_eq!(
            app.sig_index,
            (2 + COMMON_SIGNATURES.len()) % COMMON_SIGNATURES.len()
        );
    }

    #[test]
    fn set_time_signature_updates_accents() {
        let mut app = MetronomeApp::new();
        app.set_time_signature(0); // 2/4
        assert_eq!(app.accents.len(), 2);
        assert!(app.accents[0]);
    }

    // --- Toggle accent ---

    #[test]
    fn toggle_accent() {
        let mut app = MetronomeApp::new();
        assert!(!app.accents[2]);
        app.toggle_accent(2);
        assert!(app.accents[2]);
        app.toggle_accent(2);
        assert!(!app.accents[2]);
    }

    #[test]
    fn toggle_accent_out_of_bounds() {
        let mut app = MetronomeApp::new();
        app.toggle_accent(99); // should not panic
    }

    // --- Tap tempo ---

    #[test]
    fn tap_tempo_two_taps() {
        let mut app = MetronomeApp::new();
        app.tap_tempo(0);
        app.tap_tempo(500); // 500ms interval = 120 BPM
        assert_eq!(app.bpm, 120);
    }

    #[test]
    fn tap_tempo_three_taps() {
        let mut app = MetronomeApp::new();
        app.tap_tempo(0);
        app.tap_tempo(500);
        app.tap_tempo(1000); // avg interval = 500ms = 120 BPM
        assert_eq!(app.bpm, 120);
    }

    #[test]
    fn tap_tempo_single_no_change() {
        let mut app = MetronomeApp::new();
        let old_bpm = app.bpm;
        app.tap_tempo(0);
        assert_eq!(app.bpm, old_bpm);
    }

    #[test]
    fn clear_tap() {
        let mut app = MetronomeApp::new();
        app.tap_tempo(0);
        app.tap_tempo(500);
        app.clear_tap();
        assert!(app.tap_times_ms.is_empty());
    }

    #[test]
    fn tap_history_limit() {
        let mut app = MetronomeApp::new();
        for i in 0..20 {
            app.tap_tempo(i * 500);
        }
        assert!(app.tap_times_ms.len() <= TAP_HISTORY_SIZE);
    }

    // --- Play/stop ---

    #[test]
    fn toggle_play() {
        let mut app = MetronomeApp::new();
        assert!(!app.playing);
        app.toggle_play();
        assert!(app.playing);
        app.toggle_play();
        assert!(!app.playing);
    }

    #[test]
    fn play_resets_beat() {
        let mut app = MetronomeApp::new();
        app.current_beat = 3;
        app.total_beats = 100;
        app.toggle_play();
        assert_eq!(app.current_beat, 0);
        assert_eq!(app.total_beats, 0);
    }

    // --- Beat advance ---

    #[test]
    fn advance_beat_simple() {
        let mut app = MetronomeApp::new();
        app.advance_beat();
        assert_eq!(app.current_beat, 1);
        assert_eq!(app.total_beats, 1);
    }

    #[test]
    fn advance_beat_wraps_measure() {
        let mut app = MetronomeApp::new();
        for _ in 0..4 {
            app.advance_beat();
        }
        assert_eq!(app.current_beat, 0);
        assert_eq!(app.total_beats, 4);
    }

    #[test]
    fn advance_beat_with_subdivision() {
        let mut app = MetronomeApp::new();
        app.subdivision = Subdivision::Eighth;
        app.advance_beat(); // sub 0->1
        assert_eq!(app.current_sub, 1);
        assert_eq!(app.current_beat, 0);
        assert_eq!(app.total_beats, 0);
        app.advance_beat(); // sub 1->0, beat 0->1
        assert_eq!(app.current_sub, 0);
        assert_eq!(app.current_beat, 1);
        assert_eq!(app.total_beats, 1);
    }

    // --- Tick ---

    #[test]
    fn tick_not_playing() {
        let mut app = MetronomeApp::new();
        app.tick(1000);
        assert_eq!(app.current_beat, 0);
    }

    #[test]
    fn tick_advances_on_interval() {
        let mut app = MetronomeApp::new();
        app.toggle_play();
        app.last_beat_time_ms = 0;
        app.tick(501); // interval is 500ms at 120bpm
        assert_eq!(app.current_beat, 1);
        assert!(app.beat_flash_ms > 0);
    }

    #[test]
    fn tick_no_advance_before_interval() {
        let mut app = MetronomeApp::new();
        app.toggle_play();
        app.last_beat_time_ms = 0;
        app.tick(200);
        assert_eq!(app.current_beat, 0);
    }

    /// A real `Event::Tick` reaches the beat clock.
    ///
    /// Through `App::on_event` on purpose -- the front door the harness uses.
    /// `tick` was correct and had its own tests for months while the event
    /// match named only `Event::Key`, so a test that calls `tick` directly
    /// cannot tell a metronome that beats from one that sits silent.
    /// Falsified by deleting the `Event::Tick` arm and confirming this test,
    /// and only it, fails.
    #[test]
    fn a_tick_event_reaches_the_beat_clock() {
        let mut app = MetronomeApp::new();
        app.toggle_play();
        app.on_event(&Event::Tick { elapsed_ms: 501 });
        assert_eq!(app.current_beat, 1, "Event::Tick did not reach `tick`");
    }

    /// Play starts on the downbeat, not one interval later.
    #[test]
    fn pressing_play_lights_beat_one_immediately() {
        let mut app = MetronomeApp::new();
        app.toggle_play();
        assert_eq!(app.current_beat, 0);
        assert!(app.beat_flash_ms > 0, "the downbeat did not flash");
    }

    /// The beat clock does not drift when ticks land late.
    ///
    /// This is why `tick` advances `last_beat_time_ms` by one interval rather
    /// than snapping it to now.  Sixteen-millisecond frames at 120 BPM cross
    /// each 500 ms beat 4 ms late; snapping would spend that 4 ms on every
    /// beat, and after twenty beats the metronome would be most of a frame
    /// behind and still counting.
    #[test]
    fn late_ticks_do_not_make_the_tempo_drift() {
        let mut app = MetronomeApp::new();
        app.toggle_play();
        // Twenty beats at 120 BPM = 10 s, delivered as 16 ms frames.
        for _ in 0..625 {
            app.tick(16);
        }
        assert_eq!(app.now_ms, 10_000);
        assert_eq!(
            app.total_beats, 20,
            "beats drifted against the clock that produced them"
        );
    }

    /// A long gap in ticks resyncs instead of firing a burst.
    #[test]
    fn a_missed_second_does_not_fire_a_burst_of_beats() {
        let mut app = MetronomeApp::new();
        app.toggle_play();
        app.tick(10_000); // the window was not ticked for ten seconds
        assert_eq!(app.total_beats, 1, "catch-up beats were fired at the user");
        assert_eq!(
            app.last_beat_time_ms, app.now_ms,
            "the beat clock did not resync"
        );
    }

    // --- Tap tempo through the keyboard ---

    /// `T` sets the tempo from the gaps between presses.
    ///
    /// The `Key::T` arm was empty, with a comment saying tap tempo "would use
    /// system time in a real app".  It does not need one: tap tempo reads
    /// only the gaps, so the app's own tick-accumulated clock serves.
    #[test]
    fn tapping_t_four_times_sets_the_tempo() {
        let mut app = MetronomeApp::new();
        // Four taps 400 ms apart = 150 BPM.
        for _ in 0..4 {
            app.handle_key(&make_key(Key::T));
            app.tick(400);
        }
        assert_eq!(app.bpm, 150, "T did not reach tap_tempo");
    }

    /// A tap after a long pause starts a new measurement instead of poisoning
    /// the old one.
    ///
    /// Without [`TAP_STALE_MS`] the gap itself is averaged in: two taps 400 ms
    /// apart give 150 BPM, then one ten seconds later averages (400 + 10000)/2
    /// = 5200 ms, which is 11 BPM and clamps to the 20 BPM floor. The user
    /// tapped a tempo and got the slowest one the app has.
    #[test]
    fn a_tap_after_a_long_pause_does_not_average_the_pause_in() {
        let mut app = MetronomeApp::new();
        app.handle_key(&make_key(Key::T));
        app.tick(400);
        app.handle_key(&make_key(Key::T));
        assert_eq!(app.bpm, 150, "two taps 400 ms apart are 150 BPM");

        // Ten seconds of nothing, then one more tap.
        app.tick(10_000);
        app.handle_key(&make_key(Key::T));
        assert_eq!(
            app.bpm, 150,
            "the pause was averaged into the tempo instead of ending the measurement"
        );
        assert_eq!(
            app.tap_times_ms.len(),
            1,
            "the stale taps should have been forgotten, leaving only the new one"
        );
    }

    /// The frame clock is asked for only while something is moving.
    ///
    /// An app that always returns an interval keeps the compositor awake for
    /// ever: it cannot park while any window has a deadline armed. A stopped
    /// metronome with its indicator dark has nothing to advance and must say
    /// so.
    #[test]
    fn a_stopped_metronome_gives_the_clock_back() {
        let mut app = MetronomeApp::new();
        assert_eq!(
            app.tick_interval(),
            None,
            "a metronome that has never been started asked for a clock"
        );

        app.toggle_play();
        assert!(app.tick_interval().is_some(), "playing needs the clock");

        app.toggle_play();
        assert!(
            app.tick_interval().is_some(),
            "the indicator lit by the downbeat still has to go out"
        );
        app.on_event(&Event::Tick {
            elapsed_ms: BEAT_FLASH_MS + 1,
        });
        assert_eq!(
            app.tick_interval(),
            None,
            "stopped, dark and untapped, and still holding the desktop awake"
        );
    }

    /// Tap tempo's only clock is `now_ms`, so the clock must keep running
    /// between taps — otherwise every tap reads as simultaneous with the last.
    /// It is [`TAP_STALE_MS`] that lets this term expire rather than pinning
    /// the clock on for the life of the process after a single tap.
    #[test]
    fn one_tap_keeps_the_clock_until_it_goes_stale() {
        let mut app = MetronomeApp::new();
        app.handle_key(&make_key(Key::T));
        assert!(
            app.tick_interval().is_some(),
            "a tap with no clock cannot be timed against the next one"
        );

        app.on_event(&Event::Tick {
            elapsed_ms: TAP_STALE_MS + 1,
        });
        assert_eq!(
            app.tick_interval(),
            None,
            "one tap held the clock on for ever"
        );
    }

    /// A tick that changes nothing visible must not cost a frame.
    ///
    /// At 120 BPM and 60 fps that is 29 ticks in every 30. Redrawing on all of
    /// them spends a desktop's whole frame budget on a display that reads the
    /// same, which is the cost the harness's `Response::Idle` exists to avoid.
    #[test]
    fn a_tick_between_beats_asks_for_no_frame() {
        let mut app = MetronomeApp::new();
        app.toggle_play(); // 120 BPM: beats 500 ms apart, flash 150 ms.
        assert_eq!(
            app.on_event(&Event::Tick { elapsed_ms: 200 }),
            Response::Redraw,
            "the downbeat flash going out is visible and needs a frame"
        );
        assert_eq!(
            app.on_event(&Event::Tick { elapsed_ms: 16 }),
            Response::Idle,
            "a tick with the indicator already dark redrew an identical frame"
        );
        assert_eq!(
            app.on_event(&Event::Tick { elapsed_ms: 300 }),
            Response::Redraw,
            "the beat itself did not ask for a frame"
        );
    }

    /// Backspace throws away a mistimed tap history.
    #[test]
    fn backspace_clears_the_tap_history() {
        let mut app = MetronomeApp::new();
        app.handle_key(&make_key(Key::T));
        app.tick(400);
        app.handle_key(&make_key(Key::T));
        assert!(!app.tap_times_ms.is_empty());
        app.handle_key(&make_key(Key::Backspace));
        assert!(
            app.tap_times_ms.is_empty(),
            "Backspace did not clear the taps"
        );
    }

    // --- Practice mode ---

    #[test]
    fn practice_mode_toggle() {
        let mut app = MetronomeApp::new();
        assert!(!app.practice_mode);
        app.handle_key(&make_key(Key::P));
        assert!(app.practice_mode);
    }

    #[test]
    fn practice_mode_start_bpm() {
        let mut app = MetronomeApp::new();
        app.practice_mode = true;
        app.practice_start_bpm = 80;
        app.toggle_play();
        assert_eq!(app.bpm, 80);
    }

    #[test]
    fn practice_mode_increment() {
        let mut app = MetronomeApp::new();
        app.practice_mode = true;
        app.practice_start_bpm = 80;
        app.practice_target_bpm = 160;
        app.practice_increment = 10;
        app.practice_measures = 2;
        app.bpm = 80;
        // Complete 2 measures (8 beats in 4/4)
        for _ in 0..8 {
            app.advance_beat();
        }
        assert_eq!(app.bpm, 90);
    }

    // --- Key handling ---

    #[test]
    fn key_space_toggles() {
        let mut app = MetronomeApp::new();
        app.handle_key(&make_key(Key::Space));
        assert!(app.playing);
        app.handle_key(&make_key(Key::Space));
        assert!(!app.playing);
    }

    #[test]
    fn key_up_increases_bpm() {
        let mut app = MetronomeApp::new();
        app.handle_key(&make_key(Key::Up));
        assert_eq!(app.bpm, 121);
    }

    #[test]
    fn key_shift_up_increases_bpm_10() {
        let mut app = MetronomeApp::new();
        app.handle_key(&make_shift_key(Key::Up));
        assert_eq!(app.bpm, 130);
    }

    #[test]
    fn key_down_decreases_bpm() {
        let mut app = MetronomeApp::new();
        app.handle_key(&make_key(Key::Down));
        assert_eq!(app.bpm, 119);
    }

    #[test]
    fn key_s_cycles_subdivision() {
        let mut app = MetronomeApp::new();
        app.handle_key(&make_key(Key::S));
        assert_eq!(app.subdivision, Subdivision::Eighth);
    }

    #[test]
    fn key_g_cycles_time_sig() {
        let mut app = MetronomeApp::new();
        app.handle_key(&make_key(Key::G));
        assert_eq!(app.time_signature.beats_per_measure, 5);
    }

    #[test]
    fn key_r_resets() {
        let mut app = MetronomeApp::new();
        app.toggle_play();
        app.current_beat = 3;
        app.total_beats = 50;
        app.handle_key(&make_key(Key::R));
        assert!(!app.playing);
        assert_eq!(app.current_beat, 0);
        assert_eq!(app.total_beats, 0);
    }

    #[test]
    fn key_number_toggles_accent() {
        let mut app = MetronomeApp::new();
        app.handle_key(&make_key(Key::Num3));
        assert!(app.accents[2]);
    }

    #[test]
    fn key_enter_shows_settings() {
        let mut app = MetronomeApp::new();
        app.handle_key(&make_key(Key::Enter));
        assert!(app.show_settings);
    }

    #[test]
    fn key_released_ignored() {
        let mut app = MetronomeApp::new();
        app.handle_key(&KeyEvent {
            key: Key::Space,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert!(!app.playing);
    }

    // --- Settings view ---

    #[test]
    fn settings_close() {
        let mut app = MetronomeApp::new();
        app.show_settings = true;
        app.handle_key(&make_key(Key::Escape));
        assert!(!app.show_settings);
    }

    #[test]
    fn settings_adjust_target() {
        let mut app = MetronomeApp::new();
        app.show_settings = true;
        app.practice_mode = true;
        let old_target = app.practice_target_bpm;
        app.handle_key(&make_key(Key::Up));
        assert_eq!(app.practice_target_bpm, old_target + 10);
    }

    // --- Event handling ---

    #[test]
    fn on_event_routes_a_key() {
        let mut app = MetronomeApp::new();
        app.on_event(&Event::Key(make_key(Key::Space)));
        assert!(app.playing);
    }

    // --- Rendering ---

    #[test]
    fn render_main_view() {
        let app = MetronomeApp::new();
        let cmds = app.render_commands(600.0, 800.0);
        assert!(!cmds.is_empty());
        let has_title = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Metronome"));
        assert!(has_title);
    }

    #[test]
    fn render_bpm_display() {
        let app = MetronomeApp::new();
        let cmds = app.render_commands(600.0, 800.0);
        let has_bpm = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "120"));
        assert!(has_bpm);
    }

    #[test]
    fn render_playing() {
        let mut app = MetronomeApp::new();
        app.playing = true;
        let cmds = app.render_commands(600.0, 800.0);
        let has_playing = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("PLAYING")));
        assert!(has_playing);
    }

    #[test]
    fn render_settings_view() {
        let mut app = MetronomeApp::new();
        app.show_settings = true;
        let cmds = app.render_commands(600.0, 800.0);
        let has_settings = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Metronome Settings"));
        assert!(has_settings);
    }

    #[test]
    fn render_has_background() {
        let app = MetronomeApp::new();
        let cmds = app.render_commands(600.0, 800.0);
        let has_bg = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::FillRect { x, y, .. } if *x == 0.0 && *y == 0.0));
        assert!(has_bg);
    }

    #[test]
    fn render_beat_indicators() {
        let app = MetronomeApp::new();
        let cmds = app.render_commands(600.0, 800.0);
        // Should have 4 beat indicator circles (4/4 time)
        let beat_circles = cmds
            .iter()
            .filter(|c| {
                matches!(c, RenderCommand::FillRect { corner_radii, height, .. }
                if *height > 30.0 && *height < 40.0 && corner_radii.top_left > 10.0)
            })
            .count();
        assert_eq!(beat_circles, 4);
    }

    #[test]
    fn render_practice_mode() {
        let mut app = MetronomeApp::new();
        app.practice_mode = true;
        let cmds = app.render_commands(600.0, 800.0);
        let has_practice = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Practice:")));
        assert!(has_practice);
    }
}
