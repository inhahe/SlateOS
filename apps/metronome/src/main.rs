//! Slate OS Metronome
//!
//! A musical metronome with BPM control, time signature selection,
//! visual beat indicator, tap tempo, accent patterns, and subdivisions.
//!
//! **It is silent, and says so.** No application can play sound here yet
//! (`known-issues.md` -> `[E] Applications can neither record nor play
//! sound`), so the beat is shown -- a light per beat, accents in their own
//! colour -- and the window says it is not heard, before anybody starts it
//! and goes looking for a muted speaker.
//!
//! Every control answers the pointer as well as its key: the tempo steps and
//! turns under the wheel, a press on a beat accents it (all twelve of a 12/8
//! measure, where the digits reach nine), and the practice settings are rows
//! that Up and Down walk and Left and Right change.
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

use appearance::Palette;
use appearance::Surface;
use std::process::ExitCode;
use std::time::Duration;

use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::wheel;
use oswindow::app::{App, Response};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Smallest and largest speed-up practice mode will apply.
///
/// One is the smallest increment that is still an increment; fifty is a third
/// of the usable tempo range, past which "practice" is just a different tempo.
const MIN_PRACTICE_INCREMENT: u32 = 1;
/// See `MIN_PRACTICE_INCREMENT`.
const MAX_PRACTICE_INCREMENT: u32 = 50;
/// Fewest and most measures practice mode will wait before speeding up.
const MIN_PRACTICE_MEASURES: u32 = 1;
/// See `MIN_PRACTICE_MEASURES`.
const MAX_PRACTICE_MEASURES: u32 = 9;

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
// What the window says, and the keys it answers
// ---------------------------------------------------------------------------

/// What the window says under its title: this metronome is seen, not heard.
///
/// A metronome is a sound. One that flashes in silence is still worth
/// watching, but a user who hears nothing would otherwise go looking for a
/// muted speaker.
const SILENT_LINE: &str =
    "Silent: no application can play sound here yet, so the beat is shown, not heard.";

/// The keys this window answers, raised by F1 or `?`.
///
/// The main screen drew six lines of key hints and no list of its own; the
/// practice settings named theirs in their labels. One list now, and one
/// test that every key on it is answered.
const SHORTCUTS: &[(&str, &str)] = &[
    ("Space", "Start or stop"),
    ("Up / Down", "One beat a minute faster or slower"),
    ("Shift+Up / Shift+Down", "Ten faster or slower"),
    ("T", "Tap the tempo"),
    ("Backspace", "Forget the taps"),
    ("G", "Next time signature"),
    ("S", "Next subdivision"),
    ("1-9", "Accent a beat, or stop accenting it"),
    ("P", "Practice mode on or off"),
    ("R", "Stop, and count from the top"),
    (
        "Enter",
        "Practice settings: Up / Down choose, Left / Right change",
    ),
    ("F1 / ?", "This list"),
];

/// A row of the practice settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingRow {
    Practice,
    Start,
    Target,
    Increment,
    Measures,
}

/// The rows, in the order Up and Down walk them.
const SETTING_ROWS: [SettingRow; 5] = [
    SettingRow::Practice,
    SettingRow::Start,
    SettingRow::Target,
    SettingRow::Increment,
    SettingRow::Measures,
];

impl SettingRow {
    /// What the row is called.
    fn label(self) -> &'static str {
        match self {
            Self::Practice => "Practice mode",
            Self::Start => "Start at",
            Self::Target => "Speed up to",
            Self::Increment => "Speed up by",
            Self::Measures => "Every",
        }
    }
}

/// What a press can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Help,
    HelpCard,
    Play,
    Reset,
    Tap,
    ForgetTaps,
    Slower10,
    Slower,
    Faster,
    Faster10,
    /// The tempo itself: the wheel turns it.
    Bpm,
    TimeSignature,
    Subdivision,
    /// A beat of the measure, from 0: a press accents it.
    Beat(usize),
    Practice,
    Settings,
    Back,
    Row(SettingRow),
    StepBack(SettingRow),
    StepForward(SettingRow),
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
    /// The practice setting the arrows are on.
    setting_row: SettingRow,
    /// Whether the list of keys is up.
    show_help: bool,
    hover: Option<Target>,
    last_hits: Vec<(Target, Rect)>,
    wheel: wheel::Accumulator,
    /// The window's size, as last drawn.
    width: f32,
    height: f32,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
}

impl MetronomeApp {
    fn new() -> Self {
        // 4/4, and `unwrap_or` rather than an index so a shorter table
        // cannot panic the constructor. The fallback is the same signature
        // spelled out, which is what makes it a fallback rather than a lie.
        let sig = COMMON_SIGNATURES.get(2).copied().unwrap_or(TimeSignature {
            beats_per_measure: 4,
            beat_value: 4,
        });
        let mut accents = vec![false; sig.beats_per_measure as usize];
        if let Some(first) = accents.first_mut() {
            *first = true;
        }
        Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
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
            setting_row: SettingRow::Practice,
            show_help: false,
            hover: None,
            last_hits: Vec::new(),
            wheel: wheel::Accumulator::default(),
            width: 560.0,
            height: 740.0,
        }
    }

    fn beat_interval_ms(&self) -> u64 {
        if self.bpm == 0 {
            return 1000;
        }
        let sub_div = self.subdivision.subdivisions_per_beat();
        60_000u64
            .checked_div(u64::from(self.bpm).saturating_mul(u64::from(sub_div)))
            .unwrap_or(1000)
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
        // `get` rather than `len()` and an index. The bound is the same one
        // either way; the difference is that this spelling cannot drift away
        // from the access it guards, and clippy can see it.
        if let Some(&sig) = COMMON_SIGNATURES.get(idx) {
            self.sig_index = idx;
            self.time_signature = sig;
            self.accents = vec![false; self.time_signature.beats_per_measure as usize];
            if let Some(first) = self.accents.first_mut() {
                *first = true;
            }
            self.current_beat = 0;
            self.current_sub = 0;
        }
    }

    fn cycle_time_signature(&mut self) {
        let next = self
            .sig_index
            .saturating_add(1)
            .checked_rem(COMMON_SIGNATURES.len())
            .unwrap_or(0);
        self.set_time_signature(next);
    }

    fn toggle_accent(&mut self, beat: usize) {
        if let Some(accent) = self.accents.get_mut(beat) {
            *accent = !*accent;
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
                // `windows(2)` yields pairs, so both are always there --
                // but saying so with `get` costs nothing and means the
                // compiler is the one keeping the promise.
                .map(|w| match (w.first(), w.get(1)) {
                    (Some(&a), Some(&b)) => b.saturating_sub(a),
                    _ => 0,
                })
                .collect();
            let avg_interval: u64 = intervals
                .iter()
                .sum::<u64>()
                .checked_div(intervals.len() as u64)
                .unwrap_or(0);
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
        self.current_sub = self.current_sub.saturating_add(1);
        if self.current_sub >= subs {
            self.current_sub = 0;
            self.current_beat = self.current_beat.saturating_add(1);
            self.total_beats = self.total_beats.saturating_add(1);

            if self.current_beat >= self.time_signature.beats_per_measure {
                self.current_beat = 0;
                // Practice mode: increment BPM after N measures
                if self.practice_mode {
                    self.practice_measure_count = self.practice_measure_count.saturating_add(1);
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

    /// Handle a key; answers whether the window has anything new to show.
    fn handle_key(&mut self, event: &KeyEvent) -> EventResult {
        if !event.pressed {
            return EventResult::Ignored;
        }
        if event.key == Key::F1 || (event.key == Key::Slash && event.modifiers.shift) {
            self.show_help = !self.show_help;
            return EventResult::Consumed;
        }
        if self.show_help {
            // Modal: Space would start the beat from behind the card.
            if matches!(event.key, Key::Escape | Key::Enter) {
                self.show_help = false;
            }
            return EventResult::Consumed;
        }
        if self.show_settings {
            return self.handle_settings(event);
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
                self.show_settings = true;
            }
            Key::R => self.reset(),
            Key::Num1
            | Key::Num2
            | Key::Num3
            | Key::Num4
            | Key::Num5
            | Key::Num6
            | Key::Num7
            | Key::Num8
            | Key::Num9 => {
                let beat =
                    usize::try_from(digit(event.key).saturating_sub(1)).unwrap_or(usize::MAX);
                if beat >= self.accents.len() {
                    return EventResult::Ignored;
                }
                self.toggle_accent(beat);
            }
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// Stop, and count from the top.
    fn reset(&mut self) {
        self.playing = false;
        self.current_beat = 0;
        self.current_sub = 0;
        self.total_beats = 0;
        self.practice_measure_count = 0;
        self.beat_flash_ms = 0;
    }

    /// Keys on the practice settings: Up and Down choose a setting, Left and
    /// Right change it, a digit names the measures outright.
    fn handle_settings(&mut self, event: &KeyEvent) -> EventResult {
        match event.key {
            Key::Escape | Key::Enter => {
                self.show_settings = false;
            }
            Key::Up | Key::Down => {
                let at = SETTING_ROWS
                    .iter()
                    .position(|r| *r == self.setting_row)
                    .unwrap_or(0);
                let next = if event.key == Key::Down {
                    at.saturating_add(1)
                        .min(SETTING_ROWS.len().saturating_sub(1))
                } else {
                    at.saturating_sub(1)
                };
                self.setting_row = SETTING_ROWS
                    .get(next)
                    .copied()
                    .unwrap_or(SettingRow::Practice);
            }
            Key::Left | Key::Right => self.step_setting(self.setting_row, event.key == Key::Right),
            Key::Num1
            | Key::Num2
            | Key::Num3
            | Key::Num4
            | Key::Num5
            | Key::Num6
            | Key::Num7
            | Key::Num8
            | Key::Num9 => {
                // A digit names the count outright. Stepping to nine with an
                // arrow is eight keypresses for a number the user already
                // knows.
                self.practice_measures =
                    digit(event.key).clamp(MIN_PRACTICE_MEASURES, MAX_PRACTICE_MEASURES);
                self.setting_row = SettingRow::Measures;
            }
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// Change one practice setting a step.
    ///
    /// The start tempo had no writer anywhere: practice mode always began at
    /// 80, drawn as "Practice: 80 -> 160 BPM" beside settings that could be
    /// changed. A start above the target pulls the target up with it, and a
    /// target below the start pulls the start down, so the pair always
    /// describes a climb.
    fn step_setting(&mut self, row: SettingRow, forward: bool) {
        let step = |value: u32, by: u32, low: u32, high: u32| {
            if forward {
                value.saturating_add(by).min(high)
            } else {
                value.saturating_sub(by).max(low)
            }
        };
        match row {
            SettingRow::Practice => self.practice_mode = !self.practice_mode,
            SettingRow::Start => {
                self.practice_start_bpm = step(self.practice_start_bpm, 5, MIN_BPM, MAX_BPM);
                self.practice_target_bpm = self.practice_target_bpm.max(self.practice_start_bpm);
            }
            SettingRow::Target => {
                self.practice_target_bpm = step(self.practice_target_bpm, 5, MIN_BPM, MAX_BPM);
                self.practice_start_bpm = self.practice_start_bpm.min(self.practice_target_bpm);
            }
            SettingRow::Increment => {
                self.practice_increment = step(
                    self.practice_increment,
                    1,
                    MIN_PRACTICE_INCREMENT,
                    MAX_PRACTICE_INCREMENT,
                );
            }
            SettingRow::Measures => {
                self.practice_measures = step(
                    self.practice_measures,
                    1,
                    MIN_PRACTICE_MEASURES,
                    MAX_PRACTICE_MEASURES,
                );
            }
        }
    }

    /// A setting's value, as its row shows it.
    fn setting_value(&self, row: SettingRow) -> String {
        match row {
            SettingRow::Practice => String::from(if self.practice_mode { "On" } else { "Off" }),
            SettingRow::Start => format!("{} BPM", self.practice_start_bpm),
            SettingRow::Target => format!("{} BPM", self.practice_target_bpm),
            SettingRow::Increment => format!("+{} BPM", self.practice_increment),
            SettingRow::Measures => format!(
                "{} measure{}",
                self.practice_measures,
                if self.practice_measures == 1 { "" } else { "s" }
            ),
        }
    }

    // -----------------------------------------------------------------------
    // The pointer
    // -----------------------------------------------------------------------

    /// What is under `(x, y)` in the frame last shown.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if self.last_hits.is_empty() {
            return self.frame(self.width, self.height).hit_test(x, y);
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
                match self
                    .frame(self.width, self.height)
                    .hit_test(event.x, event.y)
                {
                    Some(target) => self.press(target),
                    None => EventResult::Ignored,
                }
            }
            MouseEventKind::Move => {
                let over = self.target_at(event.x, event.y);
                if over == self.hover {
                    return EventResult::Ignored;
                }
                self.hover = over;
                EventResult::Consumed
            }
            MouseEventKind::Leave => {
                if self.hover.take().is_some() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            // The wheel over the tempo turns it, a beat a minute a notch.
            MouseEventKind::Scroll { dy, .. } => {
                if self.target_at(event.x, event.y) != Some(Target::Bpm) {
                    return EventResult::Ignored;
                }
                // One a notch, whatever the rows a notch scrolls a list:
                // that setting is about lists, and a tempo is not one.
                let notches = self.wheel.rows_at(dy, 1.0);
                if notches == 0 {
                    return EventResult::Ignored;
                }
                // A notch away from the user is `dy > 0`, which `rows`
                // answers as a negative count: that is faster.
                let by = u32::try_from(notches.unsigned_abs()).unwrap_or(u32::MAX);
                if notches < 0 {
                    self.increase_bpm(by);
                } else {
                    self.decrease_bpm(by);
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// A press on `target`.
    fn press(&mut self, target: Target) -> EventResult {
        match target {
            Target::HelpCard => self.show_help = false,
            Target::Help => self.show_help = true,
            Target::Play => self.toggle_play(),
            Target::Reset => self.reset(),
            Target::Tap => self.tap_tempo(self.now_ms),
            Target::ForgetTaps => self.clear_tap(),
            Target::Slower10 => self.decrease_bpm(10),
            Target::Slower => self.decrease_bpm(1),
            Target::Faster => self.increase_bpm(1),
            Target::Faster10 => self.increase_bpm(10),
            Target::Bpm => return EventResult::Ignored,
            Target::TimeSignature => self.cycle_time_signature(),
            Target::Subdivision => self.subdivision = self.subdivision.cycle(),
            Target::Beat(i) => self.toggle_accent(i),
            Target::Practice => self.practice_mode = !self.practice_mode,
            Target::Settings => self.show_settings = true,
            Target::Back => self.show_settings = false,
            Target::Row(row) => {
                if self.setting_row == row {
                    return EventResult::Ignored;
                }
                self.setting_row = row;
            }
            Target::StepBack(row) | Target::StepForward(row) => {
                self.setting_row = row;
                self.step_setting(row, matches!(target, Target::StepForward(_)));
            }
        }
        EventResult::Consumed
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    /// The drawing itself, as a flat command list, for the tests that read
    /// what was drawn.
    ///
    /// Not named `render`: an inherent `render` beside the trait's would
    /// silently win the method lookup, and every `app.render(600.0, 800.0)`
    /// would keep compiling while testing the wrong function.
    #[cfg(test)]
    fn render_commands(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        self.frame(width, height).into_tree().commands
    }

    /// The whole window at `width` by `height`, and where every control in it
    /// is.
    fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let mut f = Frame::new(width, height);
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });
        if self.show_settings {
            self.render_settings(&mut f, width);
        } else {
            self.render_main(&mut f, width);
        }
        self.button(
            &mut f,
            Rect::new(width - 48.0, 16.0, 32.0, 28.0),
            "?",
            Target::Help,
            true,
        );
        if self.show_help {
            guitk::shortcut::render_card(
                &mut f,
                &self.palette,
                (width, height),
                0.0,
                SHORTCUTS,
                "F1 or ? closes this",
            );
            f.hit(Target::HelpCard, Rect::new(0.0, 0.0, width, height));
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
            x: guitk::text::center_x(label, rect.x + rect.w / 2.0, 13.0, FontWeightHint::Regular)
                .max(rect.x + 4.0),
            y: rect.y + (rect.h - 13.0) / 2.0,
            text: label.to_string(),
            font_size: 13.0,
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

    fn render_main(&self, f: &mut Frame<Target>, width: f32) {
        f.push(RenderCommand::Text {
            x: 30.0,
            y: 15.0,
            text: String::from("Metronome"),
            color: self.palette.ink(self.palette.lavender),
            font_size: 28.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        let (status_text, status_color) = if self.playing {
            ("● PLAYING", self.palette.green)
        } else {
            ("○ STOPPED", self.palette.overlay0)
        };
        f.push(RenderCommand::Text {
            x: 250.0,
            y: 22.0,
            text: String::from(status_text),
            color: status_color,
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        // That it is silent, before anybody starts it and hears nothing.
        f.push(RenderCommand::Text {
            x: 30.0,
            y: 54.0,
            text: String::from(SILENT_LINE),
            color: self.palette.ink(self.palette.yellow),
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((width - 60.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });

        // The tempo, which the wheel turns, and the buttons that step it.
        let card = Rect::new(30.0, 78.0, 250.0, 90.0);
        self.palette
            .push_surface(f, card.x, card.y, card.w, card.h, 12.0, Surface::Card);
        f.push(RenderCommand::Text {
            x: 60.0,
            y: 88.0,
            text: self.bpm.to_string(),
            color: self.palette.text,
            font_size: 56.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        f.push(RenderCommand::Text {
            x: 200.0,
            y: 118.0,
            text: String::from("BPM"),
            color: self.palette.subtext0,
            font_size: 18.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        f.hit(Target::Bpm, card);
        for (i, (label, target, enabled)) in [
            ("\u{2212}10", Target::Slower10, self.bpm > MIN_BPM),
            ("\u{2212}1", Target::Slower, self.bpm > MIN_BPM),
            ("+1", Target::Faster, self.bpm < MAX_BPM),
            ("+10", Target::Faster10, self.bpm < MAX_BPM),
        ]
        .into_iter()
        .enumerate()
        {
            self.button(
                f,
                Rect::new(296.0 + i as f32 * 60.0, 86.0, 54.0, 30.0),
                label,
                target,
                enabled,
            );
        }
        self.button(
            f,
            Rect::new(296.0, 126.0, 114.0, 30.0),
            "Tap",
            Target::Tap,
            true,
        );
        self.button(
            f,
            Rect::new(416.0, 126.0, 114.0, 30.0),
            "Forget taps",
            Target::ForgetTaps,
            !self.tap_times_ms.is_empty(),
        );

        f.push(RenderCommand::Text {
            x: 30.0,
            y: 176.0,
            text: String::from(tempo_name(self.bpm)),
            color: self.palette.ink(self.palette.mauve),
            font_size: 18.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // What is counted: a press steps each on, as G and S do.
        self.button(
            f,
            Rect::new(30.0, 206.0, 170.0, 30.0),
            &format!("Time: {}  \u{25B8}", self.time_signature.display()),
            Target::TimeSignature,
            true,
        );
        self.button(
            f,
            Rect::new(206.0, 206.0, 170.0, 30.0),
            &format!("Subdivision: {}  \u{25B8}", self.subdivision.name()),
            Target::Subdivision,
            true,
        );
        f.push(RenderCommand::Text {
            x: 30.0,
            y: 246.0,
            text: format!(
                "A tick every {} ms  \u{00B7}  a press on a beat accents it",
                self.beat_interval_ms()
            ),
            color: self.palette.subtext0,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((width - 60.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });

        // The beats of the measure; a press accents one, or stops accenting it
        // -- every beat, where the digits reach only the first nine.
        let beat_y = 272.0;
        let beats = self.time_signature.beats_per_measure;
        let circle_size = 36.0_f32.min(400.0 / beats as f32 - 8.0);
        let start_x = 30.0;
        for i in 0..beats {
            let cx = start_x + i as f32 * (circle_size + 8.0);
            let is_current = self.playing && i == self.current_beat && self.current_sub == 0;
            let is_accented = self.accents.get(i as usize).copied().unwrap_or(false);
            let color = if is_current && self.beat_flash_ms > 0 {
                if is_accented {
                    self.palette.red
                } else {
                    self.palette.green
                }
            } else if is_accented {
                self.palette.surface1
            } else {
                self.palette.surface0
            };
            f.push(RenderCommand::FillRect {
                x: cx,
                y: beat_y,
                width: circle_size,
                height: circle_size,
                color,
                corner_radii: CornerRadii::all(circle_size / 2.0),
            });
            f.push(RenderCommand::Text {
                x: cx + circle_size / 2.0 - 5.0,
                y: beat_y + circle_size / 2.0 - 8.0,
                text: i.saturating_add(1).to_string(),
                color: if is_current && self.beat_flash_ms > 0 {
                    self.palette.base
                } else {
                    self.palette.text
                },
                font_size: 16.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            f.hit(
                Target::Beat(i as usize),
                Rect::new(cx, beat_y, circle_size, circle_size),
            );
        }

        if self.subdivision != Subdivision::None && self.playing {
            let sub_y = beat_y + circle_size + 10.0;
            let subs = self.subdivision.subdivisions_per_beat();
            for s in 0..subs {
                let sx = start_x + s as f32 * 14.0;
                let is_current_sub = s == self.current_sub;
                f.push(RenderCommand::FillRect {
                    x: sx,
                    y: sub_y,
                    width: 10.0,
                    height: 10.0,
                    color: if is_current_sub && self.beat_flash_ms > 0 {
                        self.palette.teal
                    } else {
                        self.palette.surface0
                    },
                    corner_radii: CornerRadii::all(5.0),
                });
            }
        }

        let stats_y = beat_y + circle_size + 40.0;
        if self.playing {
            let measure = self
                .total_beats
                .checked_div(u64::from(self.time_signature.beats_per_measure))
                .unwrap_or(0)
                .saturating_add(1);
            f.push(RenderCommand::Text {
                x: 30.0,
                y: stats_y,
                text: format!(
                    "Beat: {}/{}  |  Measure: {}  |  Total beats: {}",
                    self.current_beat.saturating_add(1),
                    self.time_signature.beats_per_measure,
                    measure,
                    self.total_beats
                ),
                color: self.palette.ink(self.palette.teal),
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        if self.practice_mode {
            self.palette
                .push_surface(f, 30.0, stats_y + 25.0, 400.0, 30.0, 6.0, Surface::Card);
            f.push(RenderCommand::Text {
                x: 40.0,
                y: stats_y + 30.0,
                text: format!(
                    "Practice: {} → {} BPM (+{} every {} measures)",
                    self.practice_start_bpm,
                    self.practice_target_bpm,
                    self.practice_increment,
                    self.practice_measures
                ),
                color: self.palette.ink(self.palette.yellow),
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        let ctrl_y = stats_y + 70.0;
        self.button(
            f,
            Rect::new(30.0, ctrl_y, 120.0, 34.0),
            if self.playing {
                "\u{25A0} Stop"
            } else {
                "\u{25B6} Start"
            },
            Target::Play,
            true,
        );
        self.button(
            f,
            Rect::new(156.0, ctrl_y, 90.0, 34.0),
            "Reset",
            Target::Reset,
            true,
        );
        self.button(
            f,
            Rect::new(252.0, ctrl_y, 140.0, 34.0),
            if self.practice_mode {
                "Practice: On"
            } else {
                "Practice: Off"
            },
            Target::Practice,
            true,
        );
        self.button(
            f,
            Rect::new(398.0, ctrl_y, 120.0, 34.0),
            "Settings\u{2026}",
            Target::Settings,
            true,
        );
        f.push(RenderCommand::Text {
            x: 30.0,
            y: ctrl_y + 48.0,
            text: String::from("F1 or ? lists every key"),
            color: self.palette.subtext0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    fn render_settings(&self, f: &mut Frame<Target>, width: f32) {
        f.push(RenderCommand::Text {
            x: 30.0,
            y: 20.0,
            text: String::from("Metronome Settings"),
            color: self.palette.ink(self.palette.lavender),
            font_size: 24.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        self.button(
            f,
            Rect::new(30.0, 58.0, 80.0, 28.0),
            "Back",
            Target::Back,
            true,
        );
        f.push(RenderCommand::Text {
            x: 122.0,
            y: 64.0,
            text: String::from(
                "Up / Down choose a setting, Left / Right change it, 1-9 set the measures",
            ),
            color: self.palette.subtext0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((width - 152.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });

        for (i, row) in SETTING_ROWS.iter().enumerate() {
            let y = 100.0 + i as f32 * 46.0;
            let rect = Rect::new(30.0, y, (width - 60.0).min(470.0), 38.0);
            let chosen = self.setting_row == *row;
            self.palette.push_surface(
                f,
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                6.0,
                if chosen {
                    Surface::Selected
                } else {
                    Surface::Card
                },
            );
            f.hit(Target::Row(*row), rect);
            f.push(RenderCommand::Text {
                x: rect.x + 14.0,
                y: y + 11.0,
                text: String::from(row.label()),
                color: self.palette.text,
                font_size: 15.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(190.0),
                overflow: TextOverflow::Ellipsis,
            });
            let right = rect.x + rect.w;
            self.button(
                f,
                Rect::new(right - 190.0, y + 5.0, 28.0, 28.0),
                "\u{25C0}",
                Target::StepBack(*row),
                true,
            );
            f.push(RenderCommand::Text {
                x: guitk::text::center_x(
                    &self.setting_value(*row),
                    right - 101.0,
                    15.0,
                    FontWeightHint::Bold,
                ),
                y: y + 11.0,
                text: self.setting_value(*row),
                color: self.palette.text,
                font_size: 15.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(110.0),
                overflow: TextOverflow::Ellipsis,
            });
            self.button(
                f,
                Rect::new(right - 40.0, y + 5.0, 28.0, 28.0),
                "\u{25B6}",
                Target::StepForward(*row),
                true,
            );
        }
        f.push(RenderCommand::Text {
            x: 30.0,
            y: 100.0 + SETTING_ROWS.len() as f32 * 46.0 + 8.0,
            text: String::from(
                "Practice starts at the start tempo, and speeds up by the increase every so many measures until it reaches the target.",
            ),
            color: self.palette.subtext0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((width - 60.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

/// The number a digit key names.
fn digit(key: Key) -> u32 {
    match key {
        Key::Num1 => 1,
        Key::Num2 => 2,
        Key::Num3 => 3,
        Key::Num4 => 4,
        Key::Num5 => 5,
        Key::Num6 => 6,
        Key::Num7 => 7,
        Key::Num8 => 8,
        Key::Num9 => 9,
        _ => 0,
    }
}

impl App for MetronomeApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        String::from("Metronome")
    }

    fn initial_size(&self) -> (u32, u32) {
        (560, 740)
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
        let result = match event {
            Event::Key(ke) => self.handle_key(ke),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            // Without this the metronome never beat: `tick` was correct and
            // tested, and nothing called it. known-issues.md lesson 45, and
            // lesson 47 for the shape it takes in a GUI app — the window still
            // laid out, still repainted and still answered the keyboard while
            // showing a beat counter frozen at one.
            Event::Tick { elapsed_ms } => {
                if self.tick(*elapsed_ms) {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            // `Resize` and `ScaleChanged` are absent on purpose: the harness
            // redraws for those itself, because the frame on screen was drawn
            // at the old geometry whatever the app thinks. See
            // `oswindow::app::drive`.
            _ => EventResult::Ignored,
        };
        match result {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.width = width;
        self.height = height;
        let frame = self.frame(width, height);
        self.last_hits = frame.hits().to_vec();
        frame.into_tree()
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
    // Panicking on bad data is the point of a test, as CLAUDE.md prescribes.
    // Production code above has no indexing and no unchecked arithmetic left.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

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

    /// An app in practice mode with the settings panel open.
    fn practising() -> MetronomeApp {
        let mut app = MetronomeApp::new();
        app.handle_key(&make_key(Key::P));
        app.handle_key(&make_key(Key::Enter));
        assert!(app.practice_mode, "control: P turns practice mode on");
        assert!(app.show_settings, "control: Enter opens the settings panel");
        app
    }

    /// Right raises the speed-up practice mode applies.
    ///
    /// `practice_increment` had no writer anywhere, so practice mode always
    /// sped up by ten -- while the panel drew "Practice Increment: +10 BPM"
    /// directly beneath a line that advertises its own keys.
    #[test]
    fn right_raises_the_practice_increment() {
        let mut app = practising();
        for _ in 0..3 {
            app.handle_key(&make_key(Key::Down));
        }
        assert_eq!(app.setting_row, SettingRow::Increment);
        let before = app.practice_increment;

        app.handle_key(&make_key(Key::Right));

        assert_eq!(
            app.practice_increment,
            before + 1,
            "Right did not raise the increment"
        );
    }

    /// Left lowers it, and stops rather than wrapping to a huge number.
    #[test]
    fn left_lowers_the_increment_and_stops_at_the_bottom() {
        let mut app = practising();
        for _ in 0..3 {
            app.handle_key(&make_key(Key::Down));
        }

        for _ in 0..40 {
            app.handle_key(&make_key(Key::Left));
        }

        assert_eq!(
            app.practice_increment, MIN_PRACTICE_INCREMENT,
            "the increment ran past its floor"
        );
    }

    /// A digit names the number of measures outright.
    ///
    /// `practice_measures` had no writer either, so practice mode always sped
    /// up every four measures.
    #[test]
    fn a_digit_sets_the_practice_measures() {
        let mut app = practising();
        assert_ne!(app.practice_measures, 7, "control: 7 is not the default");

        app.handle_key(&make_key(Key::Num7));

        assert_eq!(app.practice_measures, 7, "the digit did not set the count");
    }

    /// The settings are set before practice mode is on -- they are what it
    /// will do -- and practice mode is itself the first row.
    ///
    /// The keys used to do nothing with practice mode off, which left the
    /// panel's values unchangeable until the mode they configure was already
    /// running.
    #[test]
    fn the_settings_can_be_set_before_practice_is_on() {
        let mut app = MetronomeApp::new();
        app.handle_key(&make_key(Key::Enter));
        assert!(!app.practice_mode, "control: practice mode is off");
        for _ in 0..3 {
            app.handle_key(&make_key(Key::Down));
        }
        let before = app.practice_increment;
        app.handle_key(&make_key(Key::Right));
        assert_eq!(app.practice_increment, before + 1);
        assert!(!app.practice_mode, "setting a value turned practice on");
        for _ in 0..3 {
            app.handle_key(&make_key(Key::Up));
        }
        assert_eq!(app.setting_row, SettingRow::Practice);
        app.handle_key(&make_key(Key::Right));
        assert!(app.practice_mode, "the first row turns practice on");
    }

    /// The start tempo can be set, and practice starts there.
    ///
    /// It had no writer: practice always began at 80 BPM, drawn as
    /// "Practice: 80 -> 160 BPM" beside settings that could be changed.
    #[test]
    fn practice_starts_at_the_start_tempo_that_was_set() {
        let mut app = MetronomeApp::new();
        app.handle_key(&make_key(Key::Enter));
        app.handle_key(&make_key(Key::Down));
        assert_eq!(app.setting_row, SettingRow::Start);
        for _ in 0..4 {
            app.handle_key(&make_key(Key::Right));
        }
        assert_eq!(app.practice_start_bpm, 100);
        app.handle_key(&make_key(Key::Escape));
        app.handle_key(&make_key(Key::P));
        app.handle_key(&make_key(Key::Space));
        assert_eq!(app.bpm, 100, "practice did not start at the start tempo");
    }

    /// A start above the target pulls the target up, and a target below the
    /// start pulls the start down: the pair always describes a climb.
    #[test]
    fn the_start_and_the_target_stay_in_order() {
        let mut app = MetronomeApp::new();
        app.practice_start_bpm = 150;
        app.practice_target_bpm = 150;
        app.step_setting(SettingRow::Start, true);
        assert_eq!(
            (app.practice_start_bpm, app.practice_target_bpm),
            (155, 155)
        );
        app.step_setting(SettingRow::Target, false);
        app.step_setting(SettingRow::Target, false);
        assert_eq!(
            (app.practice_start_bpm, app.practice_target_bpm),
            (145, 145)
        );
    }

    /// The panel says which keys change each of the three settings.
    #[test]
    fn the_practice_panel_names_its_keys() {
        let app = practising();
        let text: Vec<String> = app
            .render_commands(600.0, 800.0)
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();

        for hint in ["Left / Right", "1-9"] {
            assert!(
                text.iter().any(|t| t.contains(hint)),
                "the panel never says {hint}: {text:?}"
            );
        }
    }

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
        app.handle_key(&make_key(Key::Down));
        app.handle_key(&make_key(Key::Down));
        app.handle_key(&make_key(Key::Right));
        assert_eq!(app.practice_target_bpm, old_target + 5);
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

    // -- The pointer, the card and the silence -------------------------------------

    use guitk::probe::{self, Probe};

    impl Probe for MetronomeApp {
        type Target = Target;
        type Outcome = EventResult;
        const SIZE: (f32, f32) = (560.0, 740.0);

        fn draw(&self, size: (f32, f32)) -> Frame<Target> {
            self.frame(size.0, size.1)
        }

        fn click_at(
            &mut self,
            x: f32,
            y: f32,
            button: MouseButton,
            _size: (f32, f32),
        ) -> EventResult {
            self.handle_mouse(&MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            })
        }

        fn key_at(&mut self, key: &KeyEvent, _size: (f32, f32)) -> EventResult {
            self.handle_key(key)
        }

        fn scroll_at(&mut self, x: f32, y: f32, dy: f32, _size: (f32, f32)) -> Option<EventResult> {
            Some(self.handle_mouse(&MouseEvent {
                x,
                y,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            }))
        }
    }

    fn texts(app: &MetronomeApp) -> Vec<String> {
        app.render_commands(560.0, 740.0)
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// It says it is silent, before anybody starts it and hears nothing.
    #[test]
    fn the_window_says_the_beat_is_not_heard() {
        let app = MetronomeApp::new();
        assert!(texts(&app).iter().any(|t| t == SILENT_LINE));
        assert!(SILENT_LINE.contains("no application can play sound"));
    }

    /// **Every key the card advertises is answered**, on the main screen or
    /// on the practice settings.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let answered = [false, true].into_iter().any(|settings| {
                    let mut app = MetronomeApp::new();
                    app.show_settings = settings;
                    app.handle_key(&stroke) == EventResult::Consumed
                });
                assert!(
                    answered,
                    "the card advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// The card reaches the window, and nothing starts behind it.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let mut app = MetronomeApp::new();
        app.handle_key(&make_key(Key::F1));
        let missing = guitk::shortcut::missing_rows(&texts(&app), SHORTCUTS);
        assert!(missing.is_empty(), "{missing:?}");
        app.handle_key(&make_key(Key::Space));
        assert!(!app.playing, "Space started the beat through the card");
        app.handle_key(&make_key(Key::F1));
        app.handle_key(&make_key(Key::Space));
        assert!(app.playing, "control: Space starts it with the card down");
    }

    /// Every control answers the pointer.
    #[test]
    fn every_control_answers_the_pointer() {
        let mut app = MetronomeApp::new();
        probe::click(&mut app, Target::Faster);
        assert_eq!(app.bpm, 121);
        probe::click(&mut app, Target::Faster10);
        assert_eq!(app.bpm, 131);
        probe::click(&mut app, Target::Slower10);
        probe::click(&mut app, Target::Slower);
        assert_eq!(app.bpm, 120);
        probe::click(&mut app, Target::Tap);
        assert_eq!(app.tap_times_ms.len(), 1);
        probe::click(&mut app, Target::ForgetTaps);
        assert!(app.tap_times_ms.is_empty());
        assert!(
            probe::rect_of(&app, Target::ForgetTaps).is_none(),
            "nothing to forget"
        );
        probe::click(&mut app, Target::TimeSignature);
        assert_eq!(app.time_signature.display(), "5/4");
        probe::click(&mut app, Target::Subdivision);
        assert_eq!(app.subdivision, Subdivision::Eighth);
        probe::click(&mut app, Target::Beat(1));
        assert!(app.accents[1]);
        probe::click(&mut app, Target::Play);
        assert!(app.playing);
        probe::click(&mut app, Target::Reset);
        assert!(!app.playing);
        probe::click(&mut app, Target::Practice);
        assert!(app.practice_mode);
        probe::click(&mut app, Target::Help);
        assert!(app.show_help);
        probe::click(&mut app, Target::HelpCard);
        assert!(!app.show_help);
        probe::click(&mut app, Target::Settings);
        assert!(app.show_settings);
        probe::click(&mut app, Target::Row(SettingRow::Measures));
        assert_eq!(app.setting_row, SettingRow::Measures);
        probe::click(&mut app, Target::StepForward(SettingRow::Start));
        assert_eq!(
            (app.practice_start_bpm, app.setting_row),
            (85, SettingRow::Start)
        );
        probe::click(&mut app, Target::StepBack(SettingRow::Measures));
        assert_eq!(app.practice_measures, 3);
        probe::click(&mut app, Target::Back);
        assert!(!app.show_settings);
    }

    /// A press accents any beat of the measure -- the twelfth of a 12/8 too,
    /// which no digit reaches.
    #[test]
    fn a_press_accents_the_twelfth_beat() {
        let mut app = MetronomeApp::new();
        let twelve = COMMON_SIGNATURES
            .iter()
            .position(|s| s.beats_per_measure == 12)
            .unwrap();
        app.set_time_signature(twelve);
        assert!(!app.accents[11]);
        probe::click(&mut app, Target::Beat(11));
        assert!(app.accents[11]);
        assert_eq!(
            app.handle_key(&make_key(Key::Num9)),
            EventResult::Consumed,
            "control: the ninth beat has a digit"
        );
        let mut four = MetronomeApp::new();
        assert_eq!(
            four.handle_key(&make_key(Key::Num9)),
            EventResult::Ignored,
            "a digit past the measure claimed to do something"
        );
    }

    /// The wheel over the tempo turns it: away is faster.
    #[test]
    fn the_wheel_turns_the_tempo() {
        let mut app = MetronomeApp::new();
        assert_eq!(
            probe::scroll_at_point(&mut app, Target::Bpm, 1.0),
            EventResult::Consumed
        );
        assert_eq!(app.bpm, 121);
        probe::scroll_at_point(&mut app, Target::Bpm, -3.0);
        assert_eq!(app.bpm, 118);
        assert_eq!(
            probe::scroll_at_point(&mut app, Target::Play, 1.0),
            EventResult::Ignored,
            "the wheel turned the tempo from somewhere else"
        );
    }

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
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

        // Named explicitly rather than relied on from the file's own imports.
        // The sixteen applications that declare their palette inside a
        // `mod mocha` block import `Color` *there*, so it is not in scope at
        // file level at all -- and once the module is emptied and removed, the
        // import goes with it.
        use guitk::Color;

        fn fills(app: &mut MetronomeApp) -> Vec<Color> {
            // Fully qualified. Several applications also have an *inherent*
            // `render`, with different arguments, and an inherent method wins
            // resolution over a trait one -- so `app.render(w, h)` calls the
            // wrong function and fails to compile in a way that looks like the
            // trait is missing.
            oswindow::app::App::render(app, 600.0, 400.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = MetronomeApp::new();

        oswindow::app::App::theme_changed(&mut app, &theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty(), "the window drew no filled rectangles");

        oswindow::app::App::theme_changed(&mut app, &theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(
            dark, light,
            "the window drew identically on the dark and light themes, so it \
             is still painting from constants"
        );

        // High contrast is the case a hardcoded palette fails silently: the
        // user asks for maximum legibility and this window alone ignores them.
        oswindow::app::App::theme_changed(
            &mut app,
            &theme(
                appearance::ThemeMode::Dark,
                Some(appearance::HighContrastScheme::WhiteOnBlack),
            ),
        );
        assert_ne!(
            dark,
            fills(&mut app),
            "high contrast reached every other surface but not this window"
        );
    }
}
