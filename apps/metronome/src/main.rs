//! OurOS Metronome
//!
//! A musical metronome with BPM control, time signature selection,
//! visual beat indicator, tap tempo, accent patterns, and subdivisions.

#![allow(dead_code)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::fn_params_excessive_bools)]

use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, Key, KeyEvent, Modifiers};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const COL_BASE: u32 = 0x1E1E2E;
const COL_MANTLE: u32 = 0x181825;
const COL_SURFACE0: u32 = 0x313244;
const COL_SURFACE1: u32 = 0x45475A;
const COL_SURFACE2: u32 = 0x585B70;
const COL_TEXT: u32 = 0xCDD6F4;
const COL_SUBTEXT0: u32 = 0xA6ADC8;
const COL_BLUE: u32 = 0x89B4FA;
const COL_GREEN: u32 = 0xA6E3A1;
const COL_RED: u32 = 0xF38BA8;
const COL_YELLOW: u32 = 0xF9E2AF;
const COL_PEACH: u32 = 0xFAB387;
const COL_LAVENDER: u32 = 0xB4BEFE;
const COL_OVERLAY0: u32 = 0x6C7086;
const COL_TEAL: u32 = 0x94E2D5;
const COL_MAUVE: u32 = 0xCBA6F7;

const MIN_BPM: u32 = 20;
const MAX_BPM: u32 = 300;
const TAP_HISTORY_SIZE: usize = 8;

// ---------------------------------------------------------------------------
// Time signature
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TimeSignature {
    beats_per_measure: u32,
    beat_value: u32, // 4 = quarter note, 8 = eighth note
}

impl TimeSignature {
    fn new(num: u32, den: u32) -> Self {
        Self {
            beats_per_measure: num,
            beat_value: den,
        }
    }

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
// Beat state (for visual feedback)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BeatType {
    Accent,      // First beat of measure (downbeat)
    Normal,      // Regular beat
    Subdivision, // Sub-beat
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
            if avg_interval > 0 {
                let calculated_bpm = (60_000 / avg_interval) as u32;
                self.set_bpm(calculated_bpm);
            }
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
            if self.practice_mode {
                self.bpm = self.practice_start_bpm;
                self.practice_measure_count = 0;
            }
        }
    }

    fn tick(&mut self, current_ms: u64) {
        if !self.playing {
            if self.beat_flash_ms > 0 {
                self.beat_flash_ms = self.beat_flash_ms.saturating_sub(16);
            }
            return;
        }

        let interval = self.beat_interval_ms();
        let elapsed = current_ms.saturating_sub(self.last_beat_time_ms);

        if elapsed >= interval {
            self.last_beat_time_ms = current_ms;
            self.advance_beat();
            self.beat_flash_ms = 150; // flash duration
        } else if self.beat_flash_ms > 0 {
            self.beat_flash_ms = self.beat_flash_ms.saturating_sub(16);
        }
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

    fn current_beat_type(&self) -> BeatType {
        if self.current_sub > 0 {
            return BeatType::Subdivision;
        }
        if self.current_beat < self.accents.len() as u32 && self.accents[self.current_beat as usize]
        {
            BeatType::Accent
        } else {
            BeatType::Normal
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
                // Tap tempo doesn't have real time in key events, use a simulation
                // In a real app this would use system time
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
            Key::Up => {
                if self.practice_mode {
                    self.practice_target_bpm = (self.practice_target_bpm + 10).min(MAX_BPM);
                }
            }
            Key::Down => {
                if self.practice_mode {
                    self.practice_target_bpm =
                        self.practice_target_bpm.saturating_sub(10).max(MIN_BPM);
                }
            }
            _ => {}
        }
    }

    fn handle_event(&mut self, event: &Event) {
        if let Event::Key(ke) = event {
            self.handle_key(ke);
        }
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
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
        });
        cmds.push(RenderCommand::Text {
            x: 200.0,
            y: 95.0,
            text: String::from("BPM"),
            color: Color::from_hex(COL_SUBTEXT0),
            font_size: 18.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
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
            });
        }

        // Controls
        let ctrl_y = stats_y + 65.0;
        let controls = [
            "Space: Play/Stop",
            "↑/↓: BPM ±1 (Shift: ±10)",
            "S: Subdivision  |  G: Time Sig",
            "1-9: Toggle accent  |  P: Practice",
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
        });

        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 55.0,
            text: String::from("Esc/Enter: Back"),
            color: Color::from_hex(COL_OVERLAY0),
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
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
            });
        }
    }
}

fn main() {
    let _app = MetronomeApp::new();
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_key(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }
    }

    fn make_shift_key(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::shift(),
            text: None,
        }
    }

    // --- Time signature ---

    #[test]
    fn time_signature_display() {
        let ts = TimeSignature::new(4, 4);
        assert_eq!(ts.display(), "4/4");
        let ts2 = TimeSignature::new(6, 8);
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

    // --- Beat type ---

    #[test]
    fn beat_type_accent() {
        let app = MetronomeApp::new();
        assert_eq!(app.current_beat_type(), BeatType::Accent);
    }

    #[test]
    fn beat_type_normal() {
        let mut app = MetronomeApp::new();
        app.current_beat = 1;
        assert_eq!(app.current_beat_type(), BeatType::Normal);
    }

    #[test]
    fn beat_type_subdivision() {
        let mut app = MetronomeApp::new();
        app.current_sub = 1;
        assert_eq!(app.current_beat_type(), BeatType::Subdivision);
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
            text: None,
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
    fn handle_event() {
        let mut app = MetronomeApp::new();
        app.handle_event(&Event::Key(make_key(Key::Space)));
        assert!(app.playing);
    }

    // --- Rendering ---

    #[test]
    fn render_main_view() {
        let app = MetronomeApp::new();
        let cmds = app.render(600.0, 800.0);
        assert!(!cmds.is_empty());
        let has_title = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Metronome"));
        assert!(has_title);
    }

    #[test]
    fn render_bpm_display() {
        let app = MetronomeApp::new();
        let cmds = app.render(600.0, 800.0);
        let has_bpm = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "120"));
        assert!(has_bpm);
    }

    #[test]
    fn render_playing() {
        let mut app = MetronomeApp::new();
        app.playing = true;
        let cmds = app.render(600.0, 800.0);
        let has_playing = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("PLAYING")));
        assert!(has_playing);
    }

    #[test]
    fn render_settings_view() {
        let mut app = MetronomeApp::new();
        app.show_settings = true;
        let cmds = app.render(600.0, 800.0);
        let has_settings = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Metronome Settings"));
        assert!(has_settings);
    }

    #[test]
    fn render_has_background() {
        let app = MetronomeApp::new();
        let cmds = app.render(600.0, 800.0);
        let has_bg = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::FillRect { x, y, .. } if *x == 0.0 && *y == 0.0));
        assert!(has_bg);
    }

    #[test]
    fn render_beat_indicators() {
        let app = MetronomeApp::new();
        let cmds = app.render(600.0, 800.0);
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
        let cmds = app.render(600.0, 800.0);
        let has_practice = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Practice:")));
        assert!(has_practice);
    }
}
